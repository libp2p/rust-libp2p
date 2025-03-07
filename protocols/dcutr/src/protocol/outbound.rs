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

use std::io;

use asynchronous_codec::Framed;
use futures::prelude::*;
use futures_timer::Delay;
use libp2p_core::{multiaddr::Protocol, Multiaddr};
use libp2p_swarm::Stream;
use thiserror::Error;
use web_time::Instant;

use crate::{proto, PROTOCOL_NAME};

pub(crate) async fn handshake(
    stream: Stream,
    candidates: Vec<Multiaddr>,
) -> Result<Vec<Multiaddr>, Error> {
    let mut stream = Framed::new(
        stream,
        quick_protobuf_codec::Codec::new(super::MAX_MESSAGE_SIZE_BYTES),
    );

    let msg = proto::HolePunch {
        type_pb: proto::Type::CONNECT,
        ObsAddrs: candidates.into_iter().map(|a| a.to_vec()).collect(),
    };

    stream.send(msg).await?;

    let sent_time = Instant::now();

    let proto::HolePunch { type_pb, ObsAddrs } = stream
        .next()
        .await
        .ok_or(io::Error::from(io::ErrorKind::UnexpectedEof))??;

    let rtt = sent_time.elapsed();

    if !matches!(type_pb, proto::Type::CONNECT) {
        return Err(Error::Protocol(ProtocolViolation::UnexpectedTypeSync));
    }

    if ObsAddrs.is_empty() {
        return Err(Error::Protocol(ProtocolViolation::NoAddresses));
    }

    let obs_addrs = ObsAddrs
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
        .collect();

    let msg = proto::HolePunch {
        type_pb: proto::Type::SYNC,
        ObsAddrs: vec![],
    };

    stream.send(msg).await?;

    Delay::new(rtt / 2).await;

    Ok(obs_addrs)
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO error")]
    Io(#[from] io::Error),
    #[error("Remote does not support the `{PROTOCOL_NAME}` protocol")]
    Unsupported,
    #[error("Protocol error")]
    Protocol(#[from] ProtocolViolation),
}

impl From<quick_protobuf_codec::Error> for Error {
    fn from(e: quick_protobuf_codec::Error) -> Self {
        Error::Protocol(ProtocolViolation::Codec(e))
    }
}

#[derive(Debug, Error)]
pub enum ProtocolViolation {
    #[error(transparent)]
    Codec(#[from] quick_protobuf_codec::Error),
    #[error("Expected 'status' field to be set.")]
    MissingStatusField,
    #[error("Expected 'reservation' field to be set.")]
    MissingReservationField,
    #[error("Expected at least one address in reservation.")]
    NoAddresses,
    #[error("Invalid expiration timestamp in reservation.")]
    InvalidReservationExpiration,
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Unexpected message type 'sync'")]
    UnexpectedTypeSync,
    #[error("Failed to parse response type field.")]
    ParseStatusField,
}
