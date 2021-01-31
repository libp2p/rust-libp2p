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

use crate::message_proto::{circuit_relay, CircuitRelay};
use crate::protocol::incoming_destination_request::IncomingDestinationRequest;
use crate::protocol::incoming_relay_request::IncomingRelayRequest;
use crate::protocol::{Peer, PeerParseError, PROTOCOL_NAME};
use futures::{future::BoxFuture, prelude::*};
use futures_codec::Framed;
use futures::channel::oneshot;
use libp2p_core::upgrade;
use prost::Message;


use std::{convert::TryFrom, error, fmt, iter};
use unsigned_varint::codec::UviBytes;

/// Configuration for an inbound upgrade that handles requests from the remote for the relay
/// protocol.
#[derive(Debug, Clone)]
pub struct RelayListen {}

/// Outcome of the listening.
pub enum RelayRemoteRequest<TSubstream> {
    /// We have been asked to become a destination.
    DestinationRequest(IncomingDestinationRequest<TSubstream>),
    /// We have been asked to relay communications to another node.
    RelayRequest((IncomingRelayRequest<TSubstream>, oneshot::Receiver<()>)),
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
        iter::once(PROTOCOL_NAME)
    }
}

impl<TSubstream> upgrade::InboundUpgrade<TSubstream> for RelayListen
where
    TSubstream: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = RelayRemoteRequest<TSubstream>;
    type Error = RelayListenError;
    type Future = BoxFuture<'static, Result<RelayRemoteRequest<TSubstream>, RelayListenError>>;

    fn upgrade_inbound(self, substream: TSubstream, _: Self::Info) -> Self::Future {
        async move {
            let codec = UviBytes::<bytes::Bytes>::default();
            // TODO: Do we need this?
            // codec.set_max_len(MAX_ACCEPTED_MESSAGE_LEN);
            let mut substream = Framed::<TSubstream, _>::new(substream, codec);

            let msg: bytes::BytesMut = substream.next().await.unwrap().unwrap();
            let msg =std::io::Cursor::new(msg);
            let CircuitRelay {
                r#type,
                src_peer,
                dst_peer,
                code: _,
            } = CircuitRelay::decode(msg)?;

            match circuit_relay::Type::from_i32(r#type.unwrap()).unwrap() {
                circuit_relay::Type::Hop => {
                    // TODO Handle
                    let peer = Peer::try_from(dst_peer.unwrap())?;
                    let (rq, notifyee) = IncomingRelayRequest::new(substream.into_inner(), peer);
                    Ok(RelayRemoteRequest::RelayRequest((rq, notifyee)))
                }
                circuit_relay::Type::Stop => {
                    // TODO Handle
                    let peer = Peer::try_from(src_peer.unwrap())?;
                    let rq = IncomingDestinationRequest::new(substream.into_inner(), peer);
                    Ok(RelayRemoteRequest::DestinationRequest(rq))
                }
                _ => Err(RelayListenError::InvalidMessageTy),
            }
        }
        .boxed()
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
