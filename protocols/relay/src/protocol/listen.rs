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
use crate::protocol::incoming_destination_req::IncomingDestinationReq;
use crate::protocol::incoming_relay_req::IncomingRelayReq;
use crate::protocol::{Peer, PeerParseError, MAX_ACCEPTED_MESSAGE_LEN, PROTOCOL_NAME};
use asynchronous_codec::Framed;
use futures::channel::oneshot;
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::upgrade;
use prost::Message;

use std::{convert::TryFrom, iter};
use unsigned_varint::codec::UviBytes;

/// Configuration for an inbound upgrade that handles requests from the remote for the relay
/// protocol.
#[derive(Debug, Clone)]
pub struct RelayListen {}

/// Outcome of the listening.
pub enum RelayRemoteReq<TSubstream> {
    /// We have been asked to become a destination.
    DestinationReq(IncomingDestinationReq<TSubstream>),
    /// We have been asked to relay communications to another node.
    RelayReq((IncomingRelayReq<TSubstream>, oneshot::Receiver<()>)),
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
    type Output = RelayRemoteReq<TSubstream>;
    type Error = RelayListenError;
    type Future = BoxFuture<'static, Result<RelayRemoteReq<TSubstream>, RelayListenError>>;

    fn upgrade_inbound(self, substream: TSubstream, _: Self::Info) -> Self::Future {
        async move {
            let mut codec = UviBytes::<bytes::Bytes>::default();
            codec.set_max_len(MAX_ACCEPTED_MESSAGE_LEN);
            let mut substream = Framed::<TSubstream, _>::new(substream, codec);

            let msg: bytes::BytesMut = substream.next().await.unwrap().unwrap();
            let msg = std::io::Cursor::new(msg);
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
                    let (rq, notifyee) = IncomingRelayReq::new(substream, peer);
                    Ok(RelayRemoteReq::RelayReq((rq, notifyee)))
                }
                circuit_relay::Type::Stop => {
                    // TODO Handle
                    let peer = Peer::try_from(src_peer.unwrap())?;
                    let rq = IncomingDestinationReq::new(substream, peer);
                    Ok(RelayRemoteReq::DestinationReq(rq))
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
    /// Failed to parse the protobuf handshake message.
    // TODO: Rename to DecodeError
    ParseError(prost::DecodeError),
    /// Failed to parse one of the peer information in the handshake message.
    PeerParseError(PeerParseError),
    /// Received a message invalid in this context.
    InvalidMessageTy,
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
