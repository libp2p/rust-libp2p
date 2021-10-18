// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use asynchronous_codec::{Decoder, Encoder};
use bytes::{BufMut, Bytes, BytesMut};
use libp2p_core::Endpoint;
use std::{
    fmt,
    hash::{Hash, Hasher},
    io, mem,
};
use unsigned_varint::{codec, encode};

// Maximum size for a packet: 1MB as per the spec.
// Since data is entirely buffered before being dispatched, we need a limit or remotes could just
// send a 4 TB-long packet full of zeroes that we kill our process with an OOM error.
pub(crate) const MAX_FRAME_SIZE: usize = 1024 * 1024;

/// A unique identifier used by the local node for a substream.
///
/// `LocalStreamId`s are sent with frames to the remote, where
/// they are received as `RemoteStreamId`s.
///
/// > **Note**: Streams are identified by a number and a role encoded as a flag
/// > on each frame that is either odd (for receivers) or even (for initiators).
/// > `Open` frames do not have a flag, but are sent unidirectionally. As a
/// > consequence, we need to remember if a stream was initiated by us or remotely
/// > and we store the information from our point of view as a `LocalStreamId`,
/// > i.e. receiving an `Open` frame results in a local ID with role `Endpoint::Listener`,
/// > whilst sending an `Open` frame results in a local ID with role `Endpoint::Dialer`.
/// > Receiving a frame with a flag identifying the remote as a "receiver" means that
/// > we initiated the stream, so the local ID has the role `Endpoint::Dialer`.
/// > Conversely, when receiving a frame with a flag identifying the remote as a "sender",
/// > the corresponding local ID has the role `Endpoint::Listener`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct LocalStreamId {
    num: u64,
    role: Endpoint,
}

impl fmt::Display for LocalStreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.role {
            Endpoint::Dialer => write!(f, "({}/initiator)", self.num),
            Endpoint::Listener => write!(f, "({}/receiver)", self.num),
        }
    }
}

impl Hash for LocalStreamId {
    #![allow(clippy::derive_hash_xor_eq)]
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.num);
    }
}

impl nohash_hasher::IsEnabled for LocalStreamId {}

/// A unique identifier used by the remote node for a substream.
///
/// `RemoteStreamId`s are received with frames from the remote
/// and mapped by the receiver to `LocalStreamId`s via
/// [`RemoteStreamId::into_local()`].
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct RemoteStreamId {
    num: u64,
    role: Endpoint,
}

impl LocalStreamId {
    pub fn dialer(num: u64) -> Self {
        Self {
            num,
            role: Endpoint::Dialer,
        }
    }

    #[cfg(test)]
    pub fn listener(num: u64) -> Self {
        Self {
            num,
            role: Endpoint::Listener,
        }
    }

    pub fn next(self) -> Self {
        Self {
            num: self
                .num
                .checked_add(1)
                .expect("Mplex substream ID overflowed"),
            ..self
        }
    }

    #[cfg(test)]
    pub fn into_remote(self) -> RemoteStreamId {
        RemoteStreamId {
            num: self.num,
            role: !self.role,
        }
    }
}

impl RemoteStreamId {
    fn dialer(num: u64) -> Self {
        Self {
            num,
            role: Endpoint::Dialer,
        }
    }

    fn listener(num: u64) -> Self {
        Self {
            num,
            role: Endpoint::Listener,
        }
    }

    /// Converts this `RemoteStreamId` into the corresponding `LocalStreamId`
    /// that identifies the same substream.
    pub fn into_local(self) -> LocalStreamId {
        LocalStreamId {
            num: self.num,
            role: !self.role,
        }
    }
}

/// An Mplex protocol frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame<T> {
    Open { stream_id: T },
    Data { stream_id: T, data: Bytes },
    Close { stream_id: T },
    Reset { stream_id: T },
}

impl Frame<RemoteStreamId> {
    pub fn remote_id(&self) -> RemoteStreamId {
        match *self {
            Frame::Open { stream_id } => stream_id,
            Frame::Data { stream_id, .. } => stream_id,
            Frame::Close { stream_id, .. } => stream_id,
            Frame::Reset { stream_id, .. } => stream_id,
        }
    }
}

pub struct Codec {
    varint_decoder: codec::Uvi<u64>,
    decoder_state: CodecDecodeState,
}

#[derive(Debug, Clone)]
enum CodecDecodeState {
    Begin,
    HasHeader(u64),
    HasHeaderAndLen(u64, usize),
    Poisoned,
}

impl Codec {
    pub fn new() -> Codec {
        Codec {
            varint_decoder: codec::Uvi::default(),
            decoder_state: CodecDecodeState::Begin,
        }
    }
}

impl Decoder for Codec {
    type Item = Frame<RemoteStreamId>;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            match mem::replace(&mut self.decoder_state, CodecDecodeState::Poisoned) {
                CodecDecodeState::Begin => match self.varint_decoder.decode(src)? {
                    Some(header) => {
                        self.decoder_state = CodecDecodeState::HasHeader(header);
                    }
                    None => {
                        self.decoder_state = CodecDecodeState::Begin;
                        return Ok(None);
                    }
                },
                CodecDecodeState::HasHeader(header) => match self.varint_decoder.decode(src)? {
                    Some(len) => {
                        if len as usize > MAX_FRAME_SIZE {
                            let msg = format!("Mplex frame length {} exceeds maximum", len);
                            return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
                        }

                        self.decoder_state =
                            CodecDecodeState::HasHeaderAndLen(header, len as usize);
                    }
                    None => {
                        self.decoder_state = CodecDecodeState::HasHeader(header);
                        return Ok(None);
                    }
                },
                CodecDecodeState::HasHeaderAndLen(header, len) => {
                    if src.len() < len {
                        self.decoder_state = CodecDecodeState::HasHeaderAndLen(header, len);
                        let to_reserve = len - src.len();
                        src.reserve(to_reserve);
                        return Ok(None);
                    }

                    let buf = src.split_to(len);
                    let num = (header >> 3) as u64;
                    let out = match header & 7 {
                        0 => Frame::Open {
                            stream_id: RemoteStreamId::dialer(num),
                        },
                        1 => Frame::Data {
                            stream_id: RemoteStreamId::listener(num),
                            data: buf.freeze(),
                        },
                        2 => Frame::Data {
                            stream_id: RemoteStreamId::dialer(num),
                            data: buf.freeze(),
                        },
                        3 => Frame::Close {
                            stream_id: RemoteStreamId::listener(num),
                        },
                        4 => Frame::Close {
                            stream_id: RemoteStreamId::dialer(num),
                        },
                        5 => Frame::Reset {
                            stream_id: RemoteStreamId::listener(num),
                        },
                        6 => Frame::Reset {
                            stream_id: RemoteStreamId::dialer(num),
                        },
                        _ => {
                            let msg = format!("Invalid mplex header value 0x{:x}", header);
                            return Err(io::Error::new(io::ErrorKind::InvalidData, msg));
                        }
                    };

                    self.decoder_state = CodecDecodeState::Begin;
                    return Ok(Some(out));
                }

                CodecDecodeState::Poisoned => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Mplex codec poisoned",
                    ));
                }
            }
        }
    }
}

impl Encoder for Codec {
    type Item = Frame<LocalStreamId>;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let (header, data) = match item {
            Frame::Open { stream_id } => (stream_id.num << 3, Bytes::new()),
            Frame::Data {
                stream_id:
                    LocalStreamId {
                        num,
                        role: Endpoint::Listener,
                    },
                data,
            } => (num << 3 | 1, data),
            Frame::Data {
                stream_id:
                    LocalStreamId {
                        num,
                        role: Endpoint::Dialer,
                    },
                data,
            } => (num << 3 | 2, data),
            Frame::Close {
                stream_id:
                    LocalStreamId {
                        num,
                        role: Endpoint::Listener,
                    },
            } => (num << 3 | 3, Bytes::new()),
            Frame::Close {
                stream_id:
                    LocalStreamId {
                        num,
                        role: Endpoint::Dialer,
                    },
            } => (num << 3 | 4, Bytes::new()),
            Frame::Reset {
                stream_id:
                    LocalStreamId {
                        num,
                        role: Endpoint::Listener,
                    },
            } => (num << 3 | 5, Bytes::new()),
            Frame::Reset {
                stream_id:
                    LocalStreamId {
                        num,
                        role: Endpoint::Dialer,
                    },
            } => (num << 3 | 6, Bytes::new()),
        };

        let mut header_buf = encode::u64_buffer();
        let header_bytes = encode::u64(header, &mut header_buf);

        let data_len = data.as_ref().len();
        let mut data_buf = encode::usize_buffer();
        let data_len_bytes = encode::usize(data_len, &mut data_buf);

        if data_len > MAX_FRAME_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "data size exceed maximum",
            ));
        }

        dst.reserve(header_bytes.len() + data_len_bytes.len() + data_len);
        dst.put(header_bytes);
        dst.put(data_len_bytes);
        dst.put(data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_large_messages_fails() {
        let mut enc = Codec::new();
        let role = Endpoint::Dialer;
        let data = Bytes::from(&[123u8; MAX_FRAME_SIZE + 1][..]);
        let bad_msg = Frame::Data {
            stream_id: LocalStreamId { num: 123, role },
            data,
        };
        let mut out = BytesMut::new();
        match enc.encode(bad_msg, &mut out) {
            Err(e) => assert_eq!(e.to_string(), "data size exceed maximum"),
            _ => panic!("Can't send a message bigger than MAX_FRAME_SIZE"),
        }

        let data = Bytes::from(&[123u8; MAX_FRAME_SIZE][..]);
        let ok_msg = Frame::Data {
            stream_id: LocalStreamId { num: 123, role },
            data,
        };
        assert!(enc.encode(ok_msg, &mut out).is_ok());
    }

    #[test]
    fn test_60bit_stream_id() {
        // Create new codec object for encoding and decoding our frame.
        let mut codec = Codec::new();
        // Create a u64 stream ID.
        let id: u64 = u32::MAX as u64 + 1;
        let stream_id = LocalStreamId {
            num: id,
            role: Endpoint::Dialer,
        };

        // Open a new frame with that stream ID.
        let original_frame = Frame::Open { stream_id };

        // Encode that frame.
        let mut enc_frame = BytesMut::new();
        codec
            .encode(original_frame, &mut enc_frame)
            .expect("Encoding to succeed.");

        // Decode encoded frame and extract stream ID.
        let dec_string_id = codec
            .decode(&mut enc_frame)
            .expect("Decoding to succeed.")
            .map(|f| f.remote_id())
            .unwrap();

        assert_eq!(dec_string_id.num, stream_id.num);
    }
}
