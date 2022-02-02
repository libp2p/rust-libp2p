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
use futures_timer::Delay;
use libp2p_core::{multiaddr::Protocol, upgrade, Multiaddr};
use libp2p_swarm::NegotiatedSubstream;
use std::convert::TryFrom;
use std::iter;
use std::time::Instant;
use thiserror::Error;

pub struct Upgrade {
    obs_addrs: Vec<Multiaddr>,
}

impl upgrade::UpgradeInfo for Upgrade {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(super::PROTOCOL_NAME)
    }
}

impl Upgrade {
    pub fn new(obs_addrs: Vec<Multiaddr>) -> Self {
        Self { obs_addrs }
    }
}

impl upgrade::OutboundUpgrade<NegotiatedSubstream> for Upgrade {
    type Output = Connect;
    type Error = UpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, substream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        let mut substream = Framed::new(substream, super::codec::Codec::new());

        let msg = HolePunch {
            r#type: hole_punch::Type::Connect.into(),
            obs_addrs: self.obs_addrs.into_iter().map(|a| a.to_vec()).collect(),
        };

        async move {
            substream.send(msg).await?;

            let sent_time = Instant::now();

            let HolePunch { r#type, obs_addrs } =
                substream
                    .next()
                    .await
                    .ok_or(super::codec::Error::Io(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "",
                    )))??;

            let rtt = sent_time.elapsed();

            let r#type = hole_punch::Type::from_i32(r#type).ok_or(UpgradeError::ParseTypeField)?;
            match r#type {
                hole_punch::Type::Connect => {}
                hole_punch::Type::Sync => return Err(UpgradeError::UnexpectedTypeSync),
            }

            let obs_addrs = if obs_addrs.is_empty() {
                return Err(UpgradeError::NoAddresses);
            } else {
                obs_addrs
                    .into_iter()
                    .map(Multiaddr::try_from)
                    // Filter out relayed addresses.
                    .filter(|a| match a {
                        Ok(a) => !a.iter().any(|p| p == Protocol::P2pCircuit),
                        Err(_) => true,
                    })
                    .collect::<Result<Vec<Multiaddr>, _>>()
                    .map_err(|_| UpgradeError::InvalidAddrs)?
            };

            let msg = HolePunch {
                r#type: hole_punch::Type::Sync.into(),
                obs_addrs: vec![],
            };

            substream.send(msg).await?;

            Delay::new(rtt / 2).await;

            Ok(Connect { obs_addrs })
        }
        .boxed()
    }
}

pub struct Connect {
    pub obs_addrs: Vec<Multiaddr>,
}

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error("Failed to encode or decode: {0}")]
    Codec(
        #[from]
        #[source]
        super::codec::Error,
    ),
    #[error("Expected 'status' field to be set.")]
    MissingStatusField,
    #[error("Expected 'reservation' field to be set.")]
    MissingReservationField,
    #[error("Expected at least one address in reservation.")]
    NoAddresses,
    #[error("Invalid expiration timestamp in reservation.")]
    InvalidReservationExpiration,
    #[error("Invalid addresses in reservation.")]
    InvalidAddrs,
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Unexpected message type 'sync'")]
    UnexpectedTypeSync,
    #[error("Failed to parse response type field.")]
    ParseStatusField,
}
