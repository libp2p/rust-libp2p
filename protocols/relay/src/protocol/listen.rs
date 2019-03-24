// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::message::{CircuitRelay, CircuitRelay_Type};
use crate::protocol::{
    dest_request::RelayDestinationRequest, hop_request::RelayHopRequest, Peer, PeerParseError,
    MAX_ACCEPTED_MESSAGE_LEN,
};
use futures::{prelude::*, try_ready};
use libp2p_core::upgrade;
use protobuf::ProtobufError;
use std::{convert::TryFrom, error, fmt, iter};
use tokio_io::{AsyncRead, AsyncWrite};

/// Configuration for an inbound upgrade that handles requests from the remote for the relay
/// protocol.
#[derive(Debug, Clone)]
pub struct RelayListen {}

/// Outcome of the listening.
pub enum RelayRemoteRequest<TSubstream> {
    /// We have been asked to become a destination.
    DestinationRequest(RelayDestinationRequest<TSubstream>),
    /// We have been asked to relay communications to another node.
    HopRequest(RelayHopRequest<TSubstream>),
}

impl RelayListen {
    /// Builds a new `RelayListen` with default options.
    pub fn new() -> RelayListen {
        RelayListen {}
    }
}

impl upgrade::UpgradeInfo for RelayListen {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/libp2p/relay/circuit/0.1.0")
    }
}

impl<TSubstream> upgrade::InboundUpgrade<TSubstream> for RelayListen
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type Output = RelayRemoteRequest<TSubstream>;
    type Error = RelayListenError;
    type Future = RelayListenFuture<TSubstream>;

    fn upgrade_inbound(
        self,
        substream: upgrade::Negotiated<TSubstream>,
        _: Self::Info,
    ) -> Self::Future {
        let f: fn(_, _, _) -> _ = |sock, msg, ()| Ok((sock, msg));
        let fut = upgrade::read_respond(substream, MAX_ACCEPTED_MESSAGE_LEN, (), f);
        RelayListenFuture { inner: fut }
    }
}

/// Future that negotiates an inbound substream for the relay protocol.
#[must_use = "futures do nothing unless polled"]
pub struct RelayListenFuture<TSubstream> {
    /// Inner stream.
    inner: upgrade::ReadRespond<
        upgrade::Negotiated<TSubstream>,
        (),
        fn(
            upgrade::Negotiated<TSubstream>,
            Vec<u8>,
            (),
        ) -> Result<(upgrade::Negotiated<TSubstream>, Vec<u8>), RelayListenError>,
    >,
}

impl<TSubstream> Future for RelayListenFuture<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type Item = RelayRemoteRequest<TSubstream>;
    type Error = RelayListenError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let (substream, msg) = try_ready!(self.inner.poll());

        let mut msg: CircuitRelay = protobuf::parse_from_bytes(&msg)?;
        match msg.get_field_type() {
            CircuitRelay_Type::HOP => {
                let peer = Peer::try_from(msg.take_dstPeer())?;
                let rq = RelayHopRequest::new(substream, peer);
                Ok(Async::Ready(RelayRemoteRequest::HopRequest(rq)))
            }
            CircuitRelay_Type::STOP => {
                let peer = Peer::try_from(msg.take_srcPeer())?;
                let rq = RelayDestinationRequest::new(substream, peer);
                Ok(Async::Ready(RelayRemoteRequest::DestinationRequest(rq)))
            }
            _ => Err(RelayListenError::InvalidMessageTy),
        }
    }
}

/// Error while upgrading with a `RelayListen`.
#[derive(Debug)]
pub enum RelayListenError {
    /// Error while reading the message that the remote is expected to send.
    ReadError(upgrade::ReadOneError),
    /// Failed to parse the protobuf handshake message.
    ParseError(ProtobufError),
    /// Failed to parse one of the peer information in the handshake message.
    PeerParseError(PeerParseError),
    /// Received a message invalid in this context.
    InvalidMessageTy,
}

impl fmt::Display for RelayListenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RelayListenError::ReadError(ref err) => {
                write!(f, "Error while reading the handshake message: {}", err)
            }
            RelayListenError::ParseError(ref err) => {
                write!(f, "Error while parsing the handshake message: {}", err)
            }
            RelayListenError::PeerParseError(ref err) => write!(
                f,
                "Error while parsing a peer in the handshake message: {}",
                err
            ),
            RelayListenError::InvalidMessageTy => {
                write!(f, "Received a message invalid in this context.")
            }
        }
    }
}

impl error::Error for RelayListenError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            RelayListenError::ReadError(ref err) => Some(err),
            RelayListenError::ParseError(ref err) => Some(err),
            RelayListenError::PeerParseError(ref err) => Some(err),
            RelayListenError::InvalidMessageTy => None,
        }
    }
}

impl From<upgrade::ReadOneError> for RelayListenError {
    fn from(err: upgrade::ReadOneError) -> Self {
        RelayListenError::ReadError(err)
    }
}

impl From<ProtobufError> for RelayListenError {
    fn from(err: ProtobufError) -> Self {
        RelayListenError::ParseError(err)
    }
}

impl From<PeerParseError> for RelayListenError {
    fn from(err: PeerParseError) -> Self {
        RelayListenError::PeerParseError(err)
    }
}
