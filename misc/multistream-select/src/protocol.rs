// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Multistream-select protocol messages an I/O operations for
//! constructing protocol negotiation flows.
//!
//! A protocol negotiation flow is constructed by using the
//! `Stream` and `Sink` implementations of `MessageIO` and
//! `MessageReader`.

use bytes::Bytes;
use futures::prelude::*;
use std::{io, fmt, error::Error, convert::TryFrom};

/// The maximum number of supported protocols that can be processed.
const MAX_PROTOCOLS: usize = 1000;

/// The maximum length (in bytes) of a protocol name.
///
/// This limit is necessary in order to be able to unambiguously parse
/// response messages without knowledge of the corresponding request.
/// 140 comes about from 3 * 47 = 141, where 47 is the ascii/utf8
/// encoding of the `/` character and an encoded protocol name is
/// at least 3 bytes long (uvi-length followed by `/` and `\n`).
/// Hence a protocol list response message with 47 protocols is at least
/// 141 bytes long and thus such a response cannot be mistaken for a
/// single protocol response. See `Message::decode`.
const MAX_PROTOCOL_LEN: usize = 140;

/// The encoded form of a multistream-select 1.0.0 header message.
const MSG_MULTISTREAM_1_0: &[u8] = b"/multistream/1.0.0\n";
/// The encoded form of a multistream-select 1.0.0 header message.
const MSG_MULTISTREAM_1_0_LAZY: &[u8] = b"/multistream-lazy/1\n";
/// The encoded form of a multistream-select 'na' message.
const MSG_PROTOCOL_NA: &[u8] = b"na\n";
/// The encoded form of a multistream-select 'ls' message.
const MSG_LS: &[u8] = b"ls\n";

/// Supported multistream-select protocol versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    /// Version 1 of the multistream-select protocol. See [1] and [2].
    ///
    /// [1] https://github.com/libp2p/specs/blob/master/connections/README.md#protocol-negotiation
    /// [2] https://github.com/multiformats/multistream-select
    V1,
    /// A lazy variant of version 1 that is identical on the wire but delays
    /// sending of protocol negotiation data as much as possible.
    ///
    /// Delaying the sending of protocol negotiation data can result in
    /// significantly fewer network roundtrips used for the negotiation,
    /// up to 0-RTT negotiation.
    ///
    /// 0-RTT negotiation is achieved if the dialer supports only a single
    /// application protocol. In that case the dialer immedidately settles
    /// on that protocol, buffering the negotiation messages to be sent
    /// with the first round of application protocol data (or an attempt
    /// is made to read from the `Negotiated` I/O stream).
    ///
    /// A listener receiving a `V1Lazy` header will similarly delay sending
    /// of the protocol confirmation.  Though typically the listener will need
    /// to read the request data before sending its response, thus triggering
    /// sending of the protocol confirmation, which, in absence of additional
    /// buffering on lower layers will result in at least two response frames
    /// to be sent.
    ///
    /// `V1Lazy` is specific to `rust-libp2p`: While the wire protocol
    /// is identical to `V1`, delayed sending of protocol negotiation frames
    /// is only safe under the following assumptions:
    ///
    ///   1. The dialer is assumed to always send the first multistream-select
    ///      protocol message immediately after the multistream header, without
    ///      first waiting for confirmation of that header. Since the listener
    ///      delays sending the protocol confirmation, a deadlock situation may
    ///      otherwise occurs that is only resolved by a timeout. This assumption
    ///      is trivially satisfied if both peers support and use `V1Lazy`.
    ///
    ///   2. When nesting multiple protocol negotiations, the listener is either
    ///      known to support all of the dialer's optimistically chosen protocols
    ///      or there is no intermediate protocol without a payload and none of
    ///      the protocol payloads has the potential for being mistaken for a
    ///      multistream-select protocol message. This avoids rare edge-cases whereby
    ///      the listener may not recognize upgrade boundaries and erroneously
    ///      process a request despite not supporting one of the intermediate
    ///      protocols that the dialer committed to. See [1] and [2].
    ///
    /// [1]: https://github.com/multiformats/go-multistream/issues/20
    /// [2]: https://github.com/libp2p/rust-libp2p/pull/1212
    V1Lazy,
    // Draft: https://github.com/libp2p/specs/pull/95
    // V2,
}

impl Default for Version {
    fn default() -> Self {
        Version::V1
    }
}

/// A protocol (name) exchanged during protocol negotiation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Protocol(Bytes);

impl AsRef<[u8]> for Protocol {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl TryFrom<Bytes> for Protocol {
    type Error = ProtocolError;

    fn try_from(value: Bytes) -> Result<Self, Self::Error> {
        if !value.as_ref().starts_with(b"/") || value.len() > MAX_PROTOCOL_LEN {
            return Err(ProtocolError::InvalidProtocol)
        }
        Ok(Protocol(value))
    }
}

impl TryFrom<&[u8]> for Protocol {
    type Error = ProtocolError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from(Bytes::from(value))
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
    }
}

/// A multistream-select protocol message.
///
/// Multistream-select protocol messages are exchanged with the goal
/// of agreeing on a application-layer protocol to use on an I/O stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// A header message identifies the multistream-select protocol
    /// that the sender wishes to speak.
    Header(Version),
    /// A protocol message identifies a protocol request or acknowledgement.
    Protocol(Protocol),
    /// A message through which a peer requests the complete list of
    /// supported protocols from the remote.
    ListProtocols,
    /// A message listing all supported protocols of a peer.
    Protocols(Vec<Protocol>),
    /// A message signaling that a requested protocol is not available.
    NotAvailable,
}

/// Writes a single unsigned variable-length integer into `dest`.
async fn write_uvi(dest: impl AsyncWrite + Unpin, val: usize) -> Result<(), ProtocolError> {
    let mut buf = unsigned_varint::encode::usize_buffer();
    let slice = unsigned_varint::encode::usize(val, &mut buf);
    dest.write_all(&slice).await?;
    Ok(())
}

/// Reads a single unsigned variable-length integer from `src` and returns it.
///
/// Returns an error if the stream ends before we could decode one.
///
/// > **Note**: This function reads bytes one by one from the stream. You are
/// >           therefore encouraged to wrap it around some buffering layer.
async fn read_uvi(src: impl AsyncRead + Unpin) -> Result<usize, ProtocolError> {
    let mut buf = unsigned_varint::encode::usize_buffer();

    loop {
        let mut buf_index = 0;
        if buf_index == buf.len() {
            return Err(From::from(io::Error::from(io::ErrorKind::InvalidData)))
        }

        src.read_exact(&mut buf[buf_index .. buf_index + 1]).await?;

        if (buf[buf_index] & 0x80) == 0 {
            let (value, _) = unsigned_varint::decode::usize(&buf)?;
            return Ok(value);
        }

        buf_index += 1;
    }
}

impl Message {
    /// Writes a `Message` into an `AsyncWrite`. Does **not** flush.
    pub async fn encode(&self, dest: impl AsyncWrite + Unpin) -> Result<(), ProtocolError> {
        match self {
            Message::Header(Version::V1) => {
                write_uvi(&mut dest, MSG_MULTISTREAM_1_0.len()).await?;
                dest.write_all(MSG_MULTISTREAM_1_0).await?;
                Ok(())
            }
            Message::Header(Version::V1Lazy) => {
                write_uvi(&mut dest, MSG_MULTISTREAM_1_0_LAZY.len()).await?;
                dest.write_all(MSG_MULTISTREAM_1_0_LAZY).await?;
                Ok(())
            }
            Message::Protocol(p) => {
                let len = p.0.as_ref().len() + 1; // + 1 for \n
                write_uvi(&mut dest, MSG_MULTISTREAM_1_0_LAZY.len()).await?;
                dest.write_all(p.0.as_ref()).await?;
                dest.write_all(&b"\n"[..]).await?;
                Ok(())
            }
            Message::ListProtocols => {
                write_uvi(&mut dest, MSG_LS.len()).await?;
                dest.write_all(MSG_LS).await?;
                Ok(())
            }
            Message::Protocols(ps) => {
                let mut buf = unsigned_varint::encode::usize_buffer();
                let mut out_msg = Vec::from(unsigned_varint::encode::usize(ps.len(), &mut buf));
                for p in ps {
                    out_msg.extend(unsigned_varint::encode::usize(p.0.as_ref().len() + 1, &mut buf)); // +1 for '\n'
                    out_msg.extend_from_slice(p.0.as_ref());
                    out_msg.push(b'\n')
                }
                write_uvi(&mut dest, out_msg.len()).await?;
                dest.write_all(&out_msg).await?;
                Ok(())
            }
            Message::NotAvailable => {
                write_uvi(&mut dest, MSG_PROTOCOL_NA.len()).await?;
                dest.write_all(MSG_PROTOCOL_NA).await?;
                Ok(())
            }
        }
    }

    /// Reads one `Message` from an `AsyncRead`.
    pub async fn decode(mut io: impl AsyncRead + Unpin) -> Result<Message, ProtocolError> {
        let msg = {
            let len = read_uvi(&mut io).await?;
            let mut msg = vec![0; len];
            io.read_exact(&mut msg).await?;
            msg
        };

        Message::from_bytes(&msg)
    }

    /// Attempts to decode existing bytes into a message.
    pub fn from_bytes(msg: &[u8]) -> Result<Message, ProtocolError> {
        if msg == MSG_MULTISTREAM_1_0_LAZY {
            return Ok(Message::Header(Version::V1Lazy))
        }

        if msg == MSG_MULTISTREAM_1_0 {
            return Ok(Message::Header(Version::V1))
        }

        if msg.get(0) == Some(&b'/') && msg.last() == Some(&b'\n') && msg.len() <= MAX_PROTOCOL_LEN {
            return Ok(Message::Protocol(Protocol::try_from(&msg[..msg.len() - 1])?));
        }

        if msg == MSG_PROTOCOL_NA {
            return Ok(Message::NotAvailable);
        }

        if msg == MSG_LS {
            return Ok(Message::ListProtocols)
        }

        // At this point, it must be a varint number of protocols, i.e.
        // a `Protocols` message.
        let (num_protocols, mut remaining) = unsigned_varint::decode::usize(&msg)?;
        if num_protocols > MAX_PROTOCOLS {
            return Err(ProtocolError::TooManyProtocols)
        }
        let mut protocols = Vec::with_capacity(num_protocols);
        for _ in 0 .. num_protocols {
            let (len, rem) = unsigned_varint::decode::usize(remaining)?;
            if len == 0 || len > rem.len() || rem[len - 1] != b'\n' {
                return Err(ProtocolError::InvalidMessage)
            }
            let p = Protocol::try_from(Bytes::from(&rem[.. len - 1]))?;
            protocols.push(p);
            remaining = &rem[len ..]
        }

        Ok(Message::Protocols(protocols))
    }
}

/// A protocol error.
#[derive(Debug)]
pub enum ProtocolError {
    /// I/O error.
    IoError(io::Error),

    /// Received an invalid message from the remote.
    InvalidMessage,

    /// A protocol (name) is invalid.
    InvalidProtocol,

    /// Too many protocols have been returned by the remote.
    TooManyProtocols,
}

impl From<io::Error> for ProtocolError {
    fn from(err: io::Error) -> ProtocolError {
        ProtocolError::IoError(err)
    }
}

impl Into<io::Error> for ProtocolError {
    fn into(self) -> io::Error {
        if let ProtocolError::IoError(e) = self {
            return e
        }
        io::ErrorKind::InvalidData.into()
    }
}

impl From<unsigned_varint::decode::Error> for ProtocolError {
    fn from(err: unsigned_varint::decode::Error) -> ProtocolError {
        Self::from(io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
    }
}

impl Error for ProtocolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match *self {
            ProtocolError::IoError(ref err) => Some(err),
            _ => None,
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            ProtocolError::IoError(e) =>
                write!(fmt, "I/O error: {}", e),
            ProtocolError::InvalidMessage =>
                write!(fmt, "Received an invalid message."),
            ProtocolError::InvalidProtocol =>
                write!(fmt, "A protocol (name) is invalid."),
            ProtocolError::TooManyProtocols =>
                write!(fmt, "Too many protocols received.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::*;
    use rand::Rng;
    use rand::distributions::Alphanumeric;
    use std::iter;

    impl Arbitrary for Protocol {
        fn arbitrary<G: Gen>(g: &mut G) -> Protocol {
            let n = g.gen_range(1, g.size());
            let p: String = iter::repeat(())
                .map(|()| g.sample(Alphanumeric))
                .take(n)
                .collect();
            Protocol(Bytes::from(format!("/{}", p)))
        }
    }

    impl Arbitrary for Message {
        fn arbitrary<G: Gen>(g: &mut G) -> Message {
            match g.gen_range(0, 5) {
                0 => Message::Header(Version::V1),
                1 => Message::NotAvailable,
                2 => Message::ListProtocols,
                3 => Message::Protocol(Protocol::arbitrary(g)),
                4 => Message::Protocols(Vec::arbitrary(g)),
                _ => panic!()
            }
        }
    }

    #[test]
    fn encode_decode_message() {
        fn prop(msg: Message) {
            let mut buf = BytesMut::new();
            msg.encode(&mut buf).expect(&format!("Encoding message failed: {:?}", msg));
            match Message::decode(buf.freeze()) {
                Ok(m) => assert_eq!(m, msg),
                Err(e) => panic!("Decoding failed: {:?}", e)
            }
        }
        quickcheck(prop as fn(_))
    }
}

