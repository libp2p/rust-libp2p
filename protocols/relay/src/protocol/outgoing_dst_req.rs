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
use futures::future::BoxFuture;
use futures::prelude::*;
use asynchronous_codec::Framed;
use libp2p_core::{upgrade, Multiaddr, PeerId};
use prost::Message;
use std::iter;
use unsigned_varint::codec::UviBytes;

/// Ask the remote to become a destination. The upgrade succeeds if the remote accepts, and fails
/// if the remote refuses.
///
/// If we take a situation where a *source* wants to talk to a *destination* through a *relay*,
/// this struct is the message that the *relay* sends to the *destination* at initialization. The
/// parameters passed to `OutgoingDstReq::new()` are the information of the *source* and the
/// *destination* (not the information of the *relay*).
///
/// The upgrade should be performed on a substream to the *destination*.
///
/// If the upgrade succeeds, the substream is returned and we must link it with the data sent from
/// the source.
#[derive(Debug, Clone)] // TODO: better Debug
pub struct OutgoingDstReq<TUserData> {
    /// The message to send to the destination. Pre-computed.
    message: Vec<u8>,
    /// User data, passed back on success or error.
    user_data: TUserData,
}

impl<TUserData> OutgoingDstReq<TUserData> {
    /// Creates a `OutgoingDstReq`. Must pass the parameters of the message.
    ///
    /// The `user_data` is passed back in the result.
    // TODO: change parameters?
    pub(crate) fn new(
        src_id: PeerId,
        src_addr: Multiaddr,
        user_data: TUserData,
    ) -> Self {
        let message = CircuitRelay {
            r#type: Some(circuit_relay::Type::Stop.into()),
            src_peer: Some(circuit_relay::Peer {
                id: src_id.to_bytes(),
                addrs: vec![src_addr.to_vec()],
            }),
            dst_peer: None,
            code: None,
        };
        let mut encoded_msg = Vec::new();
        // TODO: handle error?
        message
            .encode(&mut encoded_msg)
            .expect("all the mandatory fields are always filled; QED");

        OutgoingDstReq {
            message: encoded_msg,
            user_data,
        }
    }
}

impl<TUserData> upgrade::UpgradeInfo for OutgoingDstReq<TUserData> {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL_NAME)
    }
}

impl<TSubstream, TUserData> upgrade::OutboundUpgrade<TSubstream>
    for OutgoingDstReq<TUserData>
where
    TSubstream: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    TUserData: Send + 'static,
{
    type Output = (TSubstream, TUserData);
    type Error = OutgoingDstReqError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, substream: TSubstream, _: Self::Info) -> Self::Future {
        let mut codec = UviBytes::default();
        codec.set_max_len(MAX_ACCEPTED_MESSAGE_LEN);

        let mut substream = Framed::new(substream, codec);

        async move {
            substream.send(std::io::Cursor::new(self.message)).await?;
            let msg = substream
                .next()
                .await
                .ok_or(OutgoingDstReqError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "",
                )))??;

            let msg = std::io::Cursor::new(msg);
            let CircuitRelay {
                r#type,
                src_peer: _,
                dst_peer: _,
                code,
            } = CircuitRelay::decode(msg)?;

            if !matches!(
                r#type
                    .map(circuit_relay::Type::from_i32)
                    .flatten()
                    .ok_or(OutgoingDstReqError::ParseTypeField)?,
                circuit_relay::Type::Status
            ) {
                return Err(OutgoingDstReqError::ExpectedStatusType);
            }

            if !matches!(
                code.map(circuit_relay::Status::from_i32)
                    .flatten()
                    .ok_or(OutgoingDstReqError::ParseStatusField)?,
                circuit_relay::Status::Success
            ) {
                return Err(OutgoingDstReqError::ExpectedSuccessStatus);
            }

            Ok((substream.into_inner(), self.user_data))
        }
        .boxed()
    }
}

#[derive(Debug)]
pub enum OutgoingDstReqError {
    DecodeError(prost::DecodeError),
    Io(std::io::Error),
    ParseTypeField,
    ParseStatusField,
    ExpectedStatusType,
    ExpectedSuccessStatus,
}

impl From<std::io::Error> for OutgoingDstReqError {
    fn from(e: std::io::Error) -> Self {
        OutgoingDstReqError::Io(e)
    }
}

impl From<prost::DecodeError> for OutgoingDstReqError {
    fn from(e: prost::DecodeError) -> Self {
        OutgoingDstReqError::DecodeError(e)
    }
}
