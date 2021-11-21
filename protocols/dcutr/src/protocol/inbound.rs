// Copyright 2021 Protocol Labs.
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

use crate::message_proto::{hole_punch, HolePunch};
use asynchronous_codec::Framed;
use bytes::BytesMut;
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::{upgrade, Multiaddr};
use libp2p_swarm::NegotiatedSubstream;
use prost::Message;
use std::convert::TryFrom;
use std::io::Cursor;
use std::iter;
use thiserror::Error;
use unsigned_varint::codec::UviBytes;

pub struct Upgrade {}

impl upgrade::UpgradeInfo for Upgrade {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(super::PROTOCOL_NAME)
    }
}

impl upgrade::InboundUpgrade<NegotiatedSubstream> for Upgrade {
    type Output = PendingConnect;
    type Error = UpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, substream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        let mut codec = UviBytes::default();
        codec.set_max_len(super::MAX_MESSAGE_SIZE_BYTES);
        let mut substream = Framed::new(substream, codec);

        async move {
            let msg: bytes::BytesMut = substream
                .next()
                .await
                .ok_or(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, ""))??;

            let HolePunch { r#type, obs_addrs } = HolePunch::decode(Cursor::new(msg))?;

            let obs_addrs = if obs_addrs.is_empty() {
                return Err(UpgradeError::NoAddresses);
            } else {
                obs_addrs
                    .into_iter()
                    .map(TryFrom::try_from)
                    .collect::<Result<Vec<Multiaddr>, _>>()
                    .map_err(|_| UpgradeError::InvalidAddrs)?
            };

            let r#type = hole_punch::Type::from_i32(r#type).ok_or(UpgradeError::ParseTypeField)?;

            match r#type {
                hole_punch::Type::Connect => {}
                hole_punch::Type::Sync => return Err(UpgradeError::UnexpectedTypeSync),
            }

            Ok(PendingConnect {
                substream,
                remote_obs_addrs: obs_addrs,
            })
        }
        .boxed()
    }
}

pub struct PendingConnect {
    substream: Framed<NegotiatedSubstream, UviBytes>,
    remote_obs_addrs: Vec<Multiaddr>,
}

impl PendingConnect {
    pub async fn accept(
        mut self,
        local_obs_addrs: Vec<Multiaddr>,
    ) -> Result<Vec<Multiaddr>, UpgradeError> {
        let msg = HolePunch {
            r#type: hole_punch::Type::Connect.into(),
            obs_addrs: local_obs_addrs.into_iter().map(|a| a.to_vec()).collect(),
        };

        let mut encoded_msg = BytesMut::new();
        msg.encode(&mut encoded_msg)
            .expect("BytesMut to have sufficient capacity.");

        self.substream.send(encoded_msg.freeze()).await?;
        let msg: bytes::BytesMut = self
            .substream
            .next()
            .await
            .ok_or(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, ""))??;

        let HolePunch { r#type, .. } = HolePunch::decode(Cursor::new(msg))?;

        let r#type = hole_punch::Type::from_i32(r#type).ok_or(UpgradeError::ParseTypeField)?;
        match r#type {
            hole_punch::Type::Connect => return Err(UpgradeError::UnexpectedTypeConnect),
            hole_punch::Type::Sync => {}
        }

        Ok(self.remote_obs_addrs)
    }
}

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error("Failed to decode response: {0}.")]
    Decode(
        #[from]
        #[source]
        prost::DecodeError,
    ),
    #[error("Io error {0}")]
    Io(
        #[from]
        #[source]
        std::io::Error,
    ),
    #[error("Expected at least one address in reservation.")]
    NoAddresses,
    #[error("Invalid addresses.")]
    InvalidAddrs,
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Unexpected message type 'sync'")]
    UnexpectedTypeSync,
}
