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

use crate::message_proto::{CircuitRelay, circuit_relay};
use crate::protocol::{
    dest_request::RelayDestinationRequest, hop_request::RelayHopRequest, Peer, PeerParseError,
    MAX_ACCEPTED_MESSAGE_LEN,
};
use futures::{prelude::*, future::BoxFuture, ready};
use libp2p_core::upgrade;
use libp2p_swarm::NegotiatedSubstream;
use std::{convert::TryFrom, error, fmt, iter};
use std::task::{Context, Poll};
use prost::Message;
use std::pin::Pin;

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
    TSubstream: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = RelayRemoteRequest<TSubstream>;
    type Error = RelayListenError;
    type Future = RelayListenFuture<TSubstream>;

    fn upgrade_inbound(
        self,
        mut substream: TSubstream,
        _: Self::Info,
    ) -> Self::Future {

        RelayListenFuture { inner: async {
            let msg = upgrade::read_one(&mut substream, MAX_ACCEPTED_MESSAGE_LEN).await?;
            Ok((substream, msg))
        }.boxed()}
    }
}

/// Future that negotiates an inbound substream for the relay protocol.
#[must_use = "futures do nothing unless polled"]
pub struct RelayListenFuture<TSubstream> {
    /// Inner stream.
    inner: BoxFuture<'static, Result<(TSubstream, Vec<u8>), RelayListenError>>,
}

impl<TSubstream> Future for RelayListenFuture<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = Result<RelayRemoteRequest<TSubstream>, RelayListenError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let (substream, msg): (TSubstream, Vec<u8>) = ready!(self.inner.poll_unpin(cx))?;

        let CircuitRelay { r#type, src_peer, dst_peer, code } = CircuitRelay::decode(&*msg)?;
        // TODO: Handle
        match circuit_relay::Type::from_i32(r#type.unwrap()).unwrap() {
            circuit_relay::Type::Hop => {
                // TODO Handle
                let peer = Peer::try_from(dst_peer.unwrap())?;
                let rq = RelayHopRequest::new(substream, peer);
                Poll::Ready(Ok(RelayRemoteRequest::HopRequest(rq)))
            }
            circuit_relay::Type::Stop => {
                // TODO Handle
                let peer = Peer::try_from(src_peer.unwrap())?;
                let rq = RelayDestinationRequest::new(substream, peer);
                Poll::Ready(Ok(RelayRemoteRequest::DestinationRequest(rq)))
            }
            _ => Poll::Ready(Err(RelayListenError::InvalidMessageTy)),
        }
    }
}

/// Error while upgrading with a `RelayListen`.
#[derive(Debug)]
pub enum RelayListenError {
    /// Error while reading the message that the remote is expected to send.
    ReadError(upgrade::ReadOneError),
    /// Failed to parse the protobuf handshake message.
    // TODO: Rename to DecodeError
    ParseError(prost::DecodeError),
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

impl From<prost::DecodeError> for RelayListenError {
    fn from(err: prost::DecodeError) -> Self {
        RelayListenError::ParseError(err)
    }
}

impl From<PeerParseError> for RelayListenError {
    fn from(err: PeerParseError) -> Self {
        RelayListenError::PeerParseError(err)
    }
}
