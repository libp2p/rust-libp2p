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
use futures_timer::Delay;
use libp2p_core::{upgrade, Multiaddr};
use libp2p_swarm::NegotiatedSubstream;
use prost::Message;
use std::convert::TryFrom;
use std::io::Cursor;
use std::iter;
use std::time::Instant;
use thiserror::Error;
use unsigned_varint::codec::UviBytes;

const PROTOCOL_NAME: &[u8; 13] = b"/libp2p/dcutr";

const MAX_MESSAGE_SIZE_BYTES: usize = 4096;

// TODO: Should this be split up in two files? Inbound and outbound?

pub struct OutboundUpgrade {
    obs_addrs: Vec<Multiaddr>,
}

impl upgrade::UpgradeInfo for OutboundUpgrade {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL_NAME)
    }
}

impl OutboundUpgrade {
    pub fn new(obs_addrs: Vec<Multiaddr>) -> Self {
        Self { obs_addrs }
    }
}

impl upgrade::OutboundUpgrade<NegotiatedSubstream> for OutboundUpgrade {
    type Output = OutboundConnect;
    type Error = OutboundUpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, substream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        let msg = HolePunch {
            r#type: hole_punch::Type::Connect.into(),
            obs_addrs: self.obs_addrs.into_iter().map(|a| a.to_vec()).collect(),
        };

        let mut encoded_msg = BytesMut::new();
        msg.encode(&mut encoded_msg)
            .expect("BytesMut to have sufficient capacity.");

        let mut codec = UviBytes::default();
        codec.set_max_len(MAX_MESSAGE_SIZE_BYTES);
        let mut substream = Framed::new(substream, codec);

        async move {
            substream.send(encoded_msg.freeze()).await?;

            let sent_time = Instant::now();

            let msg: bytes::BytesMut = substream
                .next()
                .await
                .ok_or(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, ""))??;

            let rtt = sent_time.elapsed();

            let HolePunch { r#type, obs_addrs } = HolePunch::decode(Cursor::new(msg))?;

            let r#type =
                hole_punch::Type::from_i32(r#type).ok_or(OutboundUpgradeError::ParseTypeField)?;
            match r#type {
                hole_punch::Type::Connect => {}
                hole_punch::Type::Sync => return Err(OutboundUpgradeError::UnexpectedTypeSync),
            }

            let obs_addrs = if obs_addrs.is_empty() {
                return Err(OutboundUpgradeError::NoAddresses);
            } else {
                obs_addrs
                    .into_iter()
                    .map(TryFrom::try_from)
                    .collect::<Result<Vec<Multiaddr>, _>>()
                    .map_err(|_| OutboundUpgradeError::InvalidAddrs)?
            };

            let msg = HolePunch {
                r#type: hole_punch::Type::Sync.into(),
                obs_addrs: vec![],
            };

            let mut encoded_msg = BytesMut::new();
            msg.encode(&mut encoded_msg)
                .expect("BytesMut to have sufficient capacity.");

            substream.send(encoded_msg.freeze()).await?;

            Delay::new(rtt / 2).await;

            Ok(OutboundConnect { obs_addrs })
        }
        .boxed()
    }
}

#[derive(Debug, Error)]
pub enum OutboundUpgradeError {
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

pub struct InboundUpgrade {}

impl upgrade::UpgradeInfo for InboundUpgrade {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL_NAME)
    }
}

impl upgrade::InboundUpgrade<NegotiatedSubstream> for InboundUpgrade {
    type Output = InboundPendingConnect;
    type Error = InboundUpgradeError;
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, substream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        let mut codec = UviBytes::default();
        codec.set_max_len(MAX_MESSAGE_SIZE_BYTES);
        let mut substream = Framed::new(substream, codec);

        async move {
            let msg: bytes::BytesMut = substream
                .next()
                .await
                .ok_or(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, ""))??;

            let HolePunch { r#type, obs_addrs } = HolePunch::decode(Cursor::new(msg))?;

            let obs_addrs = if obs_addrs.is_empty() {
                return Err(InboundUpgradeError::NoAddresses);
            } else {
                obs_addrs
                    .into_iter()
                    .map(TryFrom::try_from)
                    .collect::<Result<Vec<Multiaddr>, _>>()
                    .map_err(|_| InboundUpgradeError::InvalidAddrs)?
            };

            let r#type =
                hole_punch::Type::from_i32(r#type).ok_or(InboundUpgradeError::ParseTypeField)?;

            match r#type {
                hole_punch::Type::Connect => {}
                hole_punch::Type::Sync => return Err(InboundUpgradeError::UnexpectedTypeSync),
            }

            Ok(InboundPendingConnect {
                substream,
                remote_obs_addrs: obs_addrs,
            })
        }
        .boxed()
    }
}

pub struct OutboundConnect {
    pub obs_addrs: Vec<Multiaddr>,
}

pub struct InboundPendingConnect {
    substream: Framed<NegotiatedSubstream, UviBytes>,
    remote_obs_addrs: Vec<Multiaddr>,
}

impl InboundPendingConnect {
    pub async fn accept(
        mut self,
        local_obs_addrs: Vec<Multiaddr>,
    ) -> Result<Vec<Multiaddr>, InboundUpgradeError> {
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

        let r#type =
            hole_punch::Type::from_i32(r#type).ok_or(InboundUpgradeError::ParseTypeField)?;
        match r#type {
            hole_punch::Type::Connect => return Err(InboundUpgradeError::UnexpectedTypeConnect),
            hole_punch::Type::Sync => {}
        }

        Ok(self.remote_obs_addrs)
    }
}

#[derive(Debug, Error)]
pub enum InboundUpgradeError {
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
