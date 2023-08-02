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

use futures::channel::mpsc;
use futures_timer::Delay;
use thiserror::Error;

use libp2p_core::Multiaddr;

use crate::priv_client::transport;
use crate::proto;
use crate::protocol::Limit;

#[derive(Debug, Error)]
pub(crate) enum UpgradeError {
    #[error("Reservation failed")]
    ReservationFailed(#[from] ReservationFailedReason),
    #[error("Circuit failed")]
    CircuitFailed(#[from] CircuitFailedReason),
    #[error("Fatal")]
    Fatal(#[from] FatalUpgradeError),
}

impl From<quick_protobuf_codec::Error> for UpgradeError {
    fn from(error: quick_protobuf_codec::Error) -> Self {
        Self::Fatal(error.into())
    }
}

#[derive(Debug, Error)]
pub enum CircuitFailedReason {
    #[error("Remote reported resource limit exceeded.")]
    ResourceLimitExceeded,
    #[error("Relay failed to connect to destination.")]
    ConnectionFailed,
    #[error("Relay has no reservation for destination.")]
    NoReservation,
    #[error("Remote denied permission.")]
    PermissionDenied,
}

#[derive(Debug, Error)]
pub enum ReservationFailedReason {
    #[error("Reservation refused.")]
    Refused,
    #[error("Remote reported resource limit exceeded.")]
    ResourceLimitExceeded,
}

#[derive(Debug, Error)]
pub enum FatalUpgradeError {
    #[error(transparent)]
    Codec(#[from] quick_protobuf_codec::Error),
    #[error("Stream closed")]
    StreamClosed,
    #[error("Expected 'status' field to be set.")]
    MissingStatusField,
    #[error("Expected 'reservation' field to be set.")]
    MissingReservationField,
    #[error("Expected at least one address in reservation.")]
    NoAddressesInReservation,
    #[error("Invalid expiration timestamp in reservation.")]
    InvalidReservationExpiration,
    #[error("Invalid addresses in reservation.")]
    InvalidReservationAddrs,
    #[error("Failed to parse response type field.")]
    ParseTypeField,
    #[error("Unexpected message type 'connect'")]
    UnexpectedTypeConnect,
    #[error("Unexpected message type 'reserve'")]
    UnexpectedTypeReserve,
    #[error("Failed to parse response type field.")]
    ParseStatusField,
    #[error("Unexpected message status '{0:?}'")]
    UnexpectedStatus(proto::Status),
}

pub(crate) enum Output {
    Reservation {
        renewal_timeout: Delay,
        addrs: Vec<Multiaddr>,
        limit: Option<Limit>,
        to_listener: mpsc::Sender<transport::ToListenerMsg>,
    },
    Circuit {
        limit: Option<Limit>,
    },
}
