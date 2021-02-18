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
use crate::protocol::{MAX_ACCEPTED_MESSAGE_LEN, PROTOCOL_NAME};
use asynchronous_codec::{Framed, FramedParts};
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::prelude::*;
use libp2p_core::{upgrade, Multiaddr, PeerId};
use prost::Message;
use std::iter;
use unsigned_varint::codec::UviBytes;

/// Ask a remote to act as a relay.
///
/// If we take a situation where a *source* wants to talk to a *destination* through a *relay*,
/// this struct is the message that the *source* sends to the *relay* at initialization. The
/// parameters passed to `OutgoingRelayReq::new()` are the information of the *destination*
/// (not the information of the *relay*).
///
/// The upgrade should be performed on a substream to the *relay*.
///
/// If the upgrade succeeds, the substream is returned and is now a brand new connection pointing
/// to the *destination*.
pub struct OutgoingRelayReq {
    dest_id: PeerId,
    dest_address: Multiaddr,
}

impl OutgoingRelayReq {
    /// Builds a request for the target to act as a relay to a third party.
    pub fn new(dest_id: PeerId, dest_address: Multiaddr) -> Self {
        OutgoingRelayReq {
            dest_id,
            dest_address,
        }
    }
}

impl upgrade::UpgradeInfo for OutgoingRelayReq {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL_NAME)
    }
}

impl<TSubstream> upgrade::OutboundUpgrade<TSubstream> for OutgoingRelayReq
where
    TSubstream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = (super::Connection<TSubstream>, oneshot::Receiver<()>);
    type Error = OutgoingRelayReqError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, substream: TSubstream, _: Self::Info) -> Self::Future {
        let OutgoingRelayReq {
            dest_id,
            dest_address,
        } = self;

        let message = CircuitRelay {
            r#type: Some(circuit_relay::Type::Hop.into()),
            src_peer: None,
            dst_peer: Some(circuit_relay::Peer {
                id: dest_id.to_bytes(),
                addrs: vec![dest_address.to_vec()],
            }),
            code: None,
        };
        let mut encoded = Vec::new();
        message
            .encode(&mut encoded)
            .expect("all the mandatory fields are always filled; QED");

        let mut codec = UviBytes::default();
        codec.set_max_len(MAX_ACCEPTED_MESSAGE_LEN);

        let mut substream = Framed::new(substream, codec);

        async move {
            substream.send(std::io::Cursor::new(encoded)).await?;
            let msg =
                substream
                    .next()
                    .await
                    .ok_or(OutgoingRelayReqError::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "",
                    )))??;

            let msg = std::io::Cursor::new(msg);
            let CircuitRelay {
                r#type,
                src_peer,
                dst_peer,
                code,
            } = CircuitRelay::decode(msg)?;

            if !matches!(
                r#type
                    .map(circuit_relay::Type::from_i32)
                    .flatten()
                    .ok_or(OutgoingRelayReqError::ParseTypeField)?,
                circuit_relay::Type::Status
            ) {
                return Err(OutgoingRelayReqError::ExpectedStatusType);
            }

            match code
                .map(circuit_relay::Status::from_i32)
                .flatten()
                .ok_or(OutgoingRelayReqError::ParseStatusField)?
            {
                circuit_relay::Status::Success => {}
                e => return Err(OutgoingRelayReqError::ReceivedErrorStatus(e)),
            }

            if src_peer.is_some() {
                return Err(OutgoingRelayReqError::UnexpectedSrcPeerWithStatusType);
            }
            if dst_peer.is_some() {
                return Err(OutgoingRelayReqError::UnexpectedDstPeerWithStatusType);
            }

            let FramedParts {
                io,
                read_buffer,
                write_buffer,
                ..
            } = substream.into_parts();
            assert!(
                write_buffer.is_empty(),
                "Expect a flushed Framed to have empty write buffer."
            );

            let (tx, rx) = oneshot::channel();

            Ok((super::Connection::new(read_buffer.freeze(), io, tx), rx))
        }
        .boxed()
    }
}

#[derive(Debug)]
pub enum OutgoingRelayReqError {
    DecodeError(prost::DecodeError),
    Io(std::io::Error),
    ParseTypeField,
    ParseStatusField,
    ExpectedStatusType,
    UnexpectedSrcPeerWithStatusType,
    UnexpectedDstPeerWithStatusType,
    ReceivedErrorStatus(circuit_relay::Status),
}

impl From<std::io::Error> for OutgoingRelayReqError {
    fn from(e: std::io::Error) -> Self {
        OutgoingRelayReqError::Io(e)
    }
}

impl From<prost::DecodeError> for OutgoingRelayReqError {
    fn from(e: prost::DecodeError) -> Self {
        OutgoingRelayReqError::DecodeError(e)
    }
}
