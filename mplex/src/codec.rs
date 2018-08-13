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

use std::cmp;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::mem;
use bytes::{BufMut, BytesMut};
use core::Endpoint;
use tokio_io::codec::{Decoder, Encoder};
use unsigned_varint::{codec, encode};

// Arbitrary maximum size for a packet.
// Since data is entirely buffered before being dispatched, we need a limit or remotes could just
// send a 4 TB-long packet full of zeroes that we kill our process with an OOM error.
const MAX_FRAME_SIZE: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone)]
pub enum Elem {
    Open { substream_id: u32 },
    Data { substream_id: u32, endpoint: Endpoint, data: BytesMut },
    Close { substream_id: u32, endpoint: Endpoint },
    Reset { substream_id: u32, endpoint: Endpoint },
}

impl Elem {
    /// Returns the ID of the substream of the message.
    pub fn substream_id(&self) -> u32 {
        match *self {
            Elem::Open { substream_id } => substream_id,
            Elem::Data { substream_id, .. } => substream_id,
            Elem::Close { substream_id, .. } => substream_id,
            Elem::Reset { substream_id, .. } => substream_id,
        }
    }

    /// Returns true if this message is `Close` or `Reset`.
    #[inline]
    pub fn is_close_or_reset_msg(&self) -> bool {
        match self {
            Elem::Close { .. } | Elem::Reset { .. } => true,
            _ => false,
        }
    }

    /// Returns true if this message is `Open`.
    #[inline]
    pub fn is_open_msg(&self) -> bool {
        if let Elem::Open { .. } = self {
            true
        } else {
            false
        }
    }
}

pub struct Codec {
    varint_decoder: codec::Uvi<u32>,
    decoder_state: CodecDecodeState,
}

#[derive(Debug, Clone)]
enum CodecDecodeState {
    Begin,
    HasHeader(u32),
    HasHeaderAndLen(u32, usize, BytesMut),
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
    type Item = Elem;
    type Error = IoError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        loop {
            match mem::replace(&mut self.decoder_state, CodecDecodeState::Poisoned) {
                CodecDecodeState::Begin => {
                    match self.varint_decoder.decode(src)? {
                        Some(header) => {
                            self.decoder_state = CodecDecodeState::HasHeader(header);
                        },
                        None => {
                            self.decoder_state = CodecDecodeState::Begin;
                            return Ok(None);
                        },
                    }
                },
                CodecDecodeState::HasHeader(header) => {
                    match self.varint_decoder.decode(src)? {
                        Some(len) => {
                            if len as usize > MAX_FRAME_SIZE {
                                return Err(IoErrorKind::InvalidData.into());
                            }

                            self.decoder_state = CodecDecodeState::HasHeaderAndLen(header, len as usize, BytesMut::with_capacity(len as usize));
                        },
                        None => {
                            self.decoder_state = CodecDecodeState::HasHeader(header);
                            return Ok(None);
                        },
                    }
                },
                CodecDecodeState::HasHeaderAndLen(header, len, mut buf) => {
                    debug_assert!(len == 0 || buf.len() < len);
                    let to_transfer = cmp::min(src.len(), len - buf.len());

                    buf.put(src.split_to(to_transfer));    // TODO: more optimal?

                    if buf.len() < len {
                        self.decoder_state = CodecDecodeState::HasHeaderAndLen(header, len, buf);
                        return Ok(None);
                    }

                    self.decoder_state = CodecDecodeState::Begin;
                    let substream_id = (header >> 3) as u32;
                    let out = match header & 7 {
                        0 => Elem::Open { substream_id },
                        1 => Elem::Data { substream_id, endpoint: Endpoint::Listener, data: buf },
                        2 => Elem::Data { substream_id, endpoint: Endpoint::Dialer, data: buf },
                        3 => Elem::Close { substream_id, endpoint: Endpoint::Listener },
                        4 => Elem::Close { substream_id, endpoint: Endpoint::Dialer },
                        5 => Elem::Reset { substream_id, endpoint: Endpoint::Listener },
                        6 => Elem::Reset { substream_id, endpoint: Endpoint::Dialer },
                        _ => return Err(IoErrorKind::InvalidData.into()),
                    };

                    return Ok(Some(out));
                },

                CodecDecodeState::Poisoned => {
                    return Err(IoErrorKind::InvalidData.into());
                }
            }
        }
    }
}

impl Encoder for Codec {
    type Item = Elem;
    type Error = IoError;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let (header, data) = match item {
            Elem::Open { substream_id } => {
                ((substream_id as u64) << 3, BytesMut::new())
            },
            Elem::Data { substream_id, endpoint: Endpoint::Listener, data } => {
                ((substream_id as u64) << 3 | 1, data)
            },
            Elem::Data { substream_id, endpoint: Endpoint::Dialer, data } => {
                ((substream_id as u64) << 3 | 2, data)
            },
            Elem::Close { substream_id, endpoint: Endpoint::Listener } => {
                ((substream_id as u64) << 3 | 3, BytesMut::new())
            },
            Elem::Close { substream_id, endpoint: Endpoint::Dialer } => {
                ((substream_id as u64) << 3 | 4, BytesMut::new())
            },
            Elem::Reset { substream_id, endpoint: Endpoint::Listener } => {
                ((substream_id as u64) << 3 | 5, BytesMut::new())
            },
            Elem::Reset { substream_id, endpoint: Endpoint::Dialer } => {
                ((substream_id as u64) << 3 | 6, BytesMut::new())
            },
        };

        let mut header_buf = encode::u64_buffer();
        let header_bytes = encode::u64(header, &mut header_buf);

        let data_len = data.as_ref().len();
        let mut data_buf = encode::usize_buffer();
        let data_len_bytes = encode::usize(data_len, &mut data_buf);

        if data_len > MAX_FRAME_SIZE {
            return Err(IoError::new(IoErrorKind::InvalidData, "data size exceed maximum"));
        }

        dst.reserve(header_bytes.len() + data_len_bytes.len() + data_len);
        dst.put(header_bytes);
        dst.put(data_len_bytes);
        dst.put(data);
        Ok(())
    }
}
