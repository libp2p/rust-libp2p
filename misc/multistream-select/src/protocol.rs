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

use bytes::{Bytes, BytesMut, BufMut};
use crate::length_delimited::{LengthDelimited, LengthDelimitedReader};
use futures::{prelude::*, try_ready};
use log::trace;
use std::{io, fmt, error::Error, convert::TryFrom};
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint as uvi;

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

impl Message {
    /// Encodes a `Message` into its byte representation.
    pub fn encode(&self, dest: &mut BytesMut) -> Result<(), ProtocolError> {
        match self {
            Message::Header(Version::V1) => {
                dest.reserve(MSG_MULTISTREAM_1_0.len());
                dest.put(MSG_MULTISTREAM_1_0);
                Ok(())
            }
            Message::Header(Version::V1Lazy) => {
                dest.reserve(MSG_MULTISTREAM_1_0_LAZY.len());
                dest.put(MSG_MULTISTREAM_1_0_LAZY);
                Ok(())
            }
            Message::Protocol(p) => {
                let len = p.0.as_ref().len() + 1; // + 1 for \n
                dest.reserve(len);
                dest.put(p.0.as_ref());
                dest.put(&b"\n"[..]);
                Ok(())
            }
            Message::ListProtocols => {
                dest.reserve(MSG_LS.len());
                dest.put(MSG_LS);
                Ok(())
            }
            Message::Protocols(ps) => {
                let mut buf = uvi::encode::usize_buffer();
                let mut out_msg = Vec::from(uvi::encode::usize(ps.len(), &mut buf));
                for p in ps {
                    out_msg.extend(uvi::encode::usize(p.0.as_ref().len() + 1, &mut buf)); // +1 for '\n'
                    out_msg.extend_from_slice(p.0.as_ref());
                    out_msg.push(b'\n')
                }
                dest.reserve(out_msg.len());
                dest.put(out_msg);
                Ok(())
            }
            Message::NotAvailable => {
                dest.reserve(MSG_PROTOCOL_NA.len());
                dest.put(MSG_PROTOCOL_NA);
                Ok(())
            }
        }
    }

    /// Decodes a `Message` from its byte representation.
    pub fn decode(mut msg: Bytes) -> Result<Message, ProtocolError> {
        if msg == MSG_MULTISTREAM_1_0_LAZY {
            return Ok(Message::Header(Version::V1Lazy))
        }

        if msg == MSG_MULTISTREAM_1_0 {
            return Ok(Message::Header(Version::V1))
        }

        if msg.get(0) == Some(&b'/') && msg.last() == Some(&b'\n') && msg.len() <= MAX_PROTOCOL_LEN {
            let p = Protocol::try_from(msg.split_to(msg.len() - 1))?;
            return Ok(Message::Protocol(p));
        }

        if msg == MSG_PROTOCOL_NA {
            return Ok(Message::NotAvailable);
        }

        if msg == MSG_LS {
            return Ok(Message::ListProtocols)
        }

        // At this point, it must be a varint number of protocols, i.e.
        // a `Protocols` message.
        let (num_protocols, mut remaining) = uvi::decode::usize(&msg)?;
        if num_protocols > MAX_PROTOCOLS {
            return Err(ProtocolError::TooManyProtocols)
        }
        let mut protocols = Vec::with_capacity(num_protocols);
        for _ in 0 .. num_protocols {
            let (len, rem) = uvi::decode::usize(remaining)?;
            if len == 0 || len > rem.len() || rem[len - 1] != b'\n' {
                return Err(ProtocolError::InvalidMessage)
            }
            let p = Protocol::try_from(Bytes::from(&rem[.. len - 1]))?;
            protocols.push(p);
            remaining = &rem[len ..]
        }

        return Ok(Message::Protocols(protocols));
    }
}

/// A `MessageIO` implements a [`Stream`] and [`Sink`] of [`Message`]s.
pub struct MessageIO<R> {
    inner: LengthDelimited<R>,
}

impl<R> MessageIO<R> {
    /// Constructs a new `MessageIO` resource wrapping the given I/O stream.
    pub fn new(inner: R) -> MessageIO<R>
    where
        R: AsyncRead + AsyncWrite
    {
        Self { inner: LengthDelimited::new(inner) }
    }

    /// Converts the `MessageIO` into a `MessageReader`, dropping the
    /// `Message`-oriented `Sink` in favour of direct `AsyncWrite` access
    /// to the underlying I/O stream.
    ///
    /// This is typically done if further negotiation messages are expected to be
    /// received but no more messages are written, allowing the writing of
    /// follow-up protocol data to commence.
    pub fn into_reader(self) -> MessageReader<R> {
        MessageReader { inner: self.inner.into_reader() }
    }

    /// Drops the `MessageIO` resource, yielding the underlying I/O stream
    /// together with the remaining write buffer containing the protocol
    /// negotiation frame data that has not yet been written to the I/O stream.
    ///
    /// The returned remaining write buffer may be prepended to follow-up
    /// protocol data to send with a single `write`. Either way, if non-empty,
    /// the write buffer _must_ eventually be written to the I/O stream
    /// _before_ any follow-up data, in order for protocol negotiation to
    /// complete cleanly.
    ///
    /// # Panics
    ///
    /// Panics if the read buffer is not empty, meaning that an incoming
    /// protocol negotiation frame has been partially read. The read buffer
    /// is guaranteed to be empty whenever [`MessageIO::poll`] returned
    /// a message.
    pub fn into_inner(self) -> (R, BytesMut) {
        self.inner.into_inner()
    }
}

impl<R> Sink for MessageIO<R>
where
    R: AsyncWrite,
{
    type SinkItem = Message;
    type SinkError = ProtocolError;

    fn start_send(&mut self, msg: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        let mut buf = BytesMut::new();
        msg.encode(&mut buf)?;
        match self.inner.start_send(buf.freeze())? {
            AsyncSink::NotReady(_) => Ok(AsyncSink::NotReady(msg)),
            AsyncSink::Ready => Ok(AsyncSink::Ready),
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(self.inner.poll_complete()?)
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        Ok(self.inner.close()?)
    }
}

impl<R> Stream for MessageIO<R>
where
    R: AsyncRead
{
    type Item = Message;
    type Error = ProtocolError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        poll_stream(&mut self.inner)
    }
}

/// A `MessageReader` implements a `Stream` of `Message`s on an underlying
/// I/O resource combined with direct `AsyncWrite` access.
#[derive(Debug)]
pub struct MessageReader<R> {
    inner: LengthDelimitedReader<R>
}

impl<R> MessageReader<R> {
    /// Drops the `MessageReader` resource, yielding the underlying I/O stream
    /// together with the remaining write buffer containing the protocol
    /// negotiation frame data that has not yet been written to the I/O stream.
    ///
    /// The returned remaining write buffer may be prepended to follow-up
    /// protocol data to send with a single `write`. Either way, if non-empty,
    /// the write buffer _must_ eventually be written to the I/O stream
    /// _before_ any follow-up data, in order for protocol negotiation to
    /// complete cleanly.
    ///
    /// # Panics
    ///
    /// Panics if the read buffer is not empty, meaning that an incoming
    /// protocol negotiation frame has been partially read. The read buffer
    /// is guaranteed to be empty whenever [`MessageReader::poll`] returned
    /// a message.
    pub fn into_inner(self) -> (R, BytesMut) {
        self.inner.into_inner()
    }

    /// Returns a reference to the underlying I/O stream.
    pub fn inner_ref(&self) -> &R {
        self.inner.inner_ref()
    }
}

impl<R> Stream for MessageReader<R>
where
    R: AsyncRead
{
    type Item = Message;
    type Error = ProtocolError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        poll_stream(&mut self.inner)
    }
}

impl<R> io::Write for MessageReader<R>
where
    R: AsyncWrite
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<TInner> AsyncWrite for MessageReader<TInner>
where
    TInner: AsyncWrite
{
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.inner.shutdown()
    }
}

fn poll_stream<S>(stream: &mut S) -> Poll<Option<Message>, ProtocolError>
where
    S: Stream<Item = Bytes, Error = io::Error>,
{
    let msg = if let Some(msg) = try_ready!(stream.poll()) {
        Message::decode(msg)?
    } else {
        return Ok(Async::Ready(None))
    };

    trace!("Received message: {:?}", msg);

    Ok(Async::Ready(Some(msg)))
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
        return io::ErrorKind::InvalidData.into()
    }
}

impl From<uvi::decode::Error> for ProtocolError {
    fn from(err: uvi::decode::Error) -> ProtocolError {
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

