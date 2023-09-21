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

use crate::proto;
use asynchronous_codec::Framed;
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::{multiaddr::Protocol, upgrade, Multiaddr};
use libp2p_swarm::{Stream, StreamProtocol};
use std::convert::TryFrom;
use std::iter;
use thiserror::Error;

pub struct Upgrade {}

impl upgrade::UpgradeInfo for Upgrade {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(super::PROTOCOL_NAME)
    }
}

impl upgrade::InboundUpgrade<Stream> for Upgrade {
    type Output = PendingConnect;
    type Error = UpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, substream: Stream, _: Self::Info) -> Self::Future {
        let mut substream = Framed::new(
            substream,
            quick_protobuf_codec::Codec::new(super::MAX_MESSAGE_SIZE_BYTES),
        );

        async move {
            let proto::HolePunch { type_pb, ObsAddrs } =
                substream.next().await.ok_or(UpgradeError::StreamClosed)??;

            let obs_addrs = if ObsAddrs.is_empty() {
                return Err(UpgradeError::NoAddresses);
            } else {
                ObsAddrs
                    .into_iter()
                    .filter_map(|a| match Multiaddr::try_from(a.to_vec()) {
                        Ok(a) => Some(a),
                        Err(e) => {
                            tracing::debug!("Unable to parse multiaddr: {e}");
                            None
                        }
                    })
                    // Filter out relayed addresses.
                    .filter(|a| {
                        if a.iter().any(|p| p == Protocol::P2pCircuit) {
                            tracing::debug!(address=%a, "Dropping relayed address");
                            false
                        } else {
                            true
                        }
                    })
                    .collect::<Vec<Multiaddr>>()
            };

            match type_pb {
                proto::Type::CONNECT => {}
                proto::Type::SYNC => return Err(UpgradeError::UnexpectedTypeSync),
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
    substream: Framed<Stream, quick_protobuf_codec::Codec<proto::HolePunch>>,
    remote_obs_addrs: Vec<Multiaddr>,
}

impl PendingConnect {
    pub async fn accept(
        mut self,
        local_obs_addrs: Vec<Multiaddr>,
    ) -> Result<Vec<Multiaddr>, UpgradeError> {
        let msg = proto::HolePunch {
            type_pb: proto::Type::CONNECT,
            ObsAddrs: local_obs_addrs.into_iter().map(|a| a.to_vec()).collect(),
        };

        self.substream.send(msg).await?;
        let proto::HolePunch { type_pb, .. } = self
            .substream
            .next()
            .await
            .ok_or(UpgradeError::StreamClosed)??;

        match type_pb {
            proto::Type::CONNECT => return Err(UpgradeError::UnexpectedTypeConnect),
            proto::Type::SYNC => {}
        }

        Ok(self.remote_obs_addrs)
    }
}

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error(transparent)]
    Codec(#[from] quick_protobuf_codec::Error),
    #[error("Stream closed")]
    StreamClosed,
    #[error("Expected at least one address in reservation.")]
    NoAddresses,
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Unexpected message type 'sync'")]
    UnexpectedTypeSync,
}
