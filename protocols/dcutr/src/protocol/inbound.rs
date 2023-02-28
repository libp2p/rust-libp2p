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
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::{multiaddr::Protocol, upgrade, Multiaddr};
use libp2p_swarm::NegotiatedSubstream;
use std::convert::TryFrom;
use std::iter;
use thiserror::Error;

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
        let mut substream = Framed::new(
            substream,
            prost_codec::Codec::new(super::MAX_MESSAGE_SIZE_BYTES),
        );

        async move {
            let HolePunch { r#type, obs_addrs } =
                substream.next().await.ok_or(UpgradeError::StreamClosed)??;

            let obs_addrs = if obs_addrs.is_empty() {
                return Err(UpgradeError::NoAddresses);
            } else {
                obs_addrs
                    .into_iter()
                    .filter_map(|a| match Multiaddr::try_from(a) {
                        Ok(a) => Some(a),
                        Err(e) => {
                            log::debug!("Unable to parse multiaddr: {e}");
                            None
                        }
                    })
                    // Filter out relayed addresses.
                    .filter(|a| {
                        if a.iter().any(|p| p == Protocol::P2pCircuit) {
                            log::debug!("Dropping relayed address {a}");
                            false
                        } else {
                            true
                        }
                    })
                    .collect::<Vec<Multiaddr>>()
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
    substream: Framed<NegotiatedSubstream, prost_codec::Codec<HolePunch>>,
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

        self.substream.send(msg).await?;
        let HolePunch { r#type, .. } = self
            .substream
            .next()
            .await
            .ok_or(UpgradeError::StreamClosed)??;

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
    #[error(transparent)]
    Codec(#[from] prost_codec::Error),
    #[error("Stream closed")]
    StreamClosed,
    #[error("Expected at least one address in reservation.")]
    NoAddresses,
    #[deprecated(since = "0.8.1", note = "Error is no longer constructed.")]
    #[error("Invalid addresses.")]
    InvalidAddrs,
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Unexpected message type 'sync'")]
    UnexpectedTypeSync,
}
