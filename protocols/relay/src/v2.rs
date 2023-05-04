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

//! Implementation of the [libp2p circuit relay v2
//! specification](https://github.com/libp2p/specs/blob/master/relay/circuit-v2.md).

pub mod client {
    #[deprecated(since = "0.15.0", note = "Use libp2p_relay::client::Event instead.")]
    pub type Event = crate::client::Event;

    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::client::Behaviour instead."
    )]
    pub type Client = crate::client::Behaviour;

    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::client::Connection instead."
    )]
    pub type RelayedConnection = crate::client::Connection;

    pub mod transport {
        use futures::future::BoxFuture;

        #[deprecated(
            since = "0.15.0",
            note = "Use libp2p_relay::client::Transport instead."
        )]
        pub type ClientTransport = crate::client::Transport;

        #[deprecated(
            since = "0.15.0",
            note = "RelayListener will become crate-private in the future
            as it shouldn't be required by end users."
        )]
        pub type RelayListener = crate::priv_client::transport::Listener;

        #[deprecated(
            since = "0.15.0",
            note = "RelayedDial type alias will be deprecated,
            users should create the alias themselves if needed."
        )]
        pub type RelayedDial = BoxFuture<
            'static,
            Result<crate::client::Connection, crate::priv_client::transport::Error>,
        >;

        #[deprecated(
            since = "0.15.0",
            note = "Use libp2p_relay::client::transport::Error instead."
        )]
        pub type RelayError = crate::client::transport::Error;

        #[deprecated(
            since = "0.15.0",
            note = "TransportToBehaviourMsg will become crate-private in the future
            as it shouldn't be required by end users."
        )]
        pub type TransportToBehaviourMsg = crate::priv_client::transport::TransportToBehaviourMsg;

        #[deprecated(
            since = "0.15.0",
            note = "ToListenerMsg will become crate-private in the future
            as it shouldn't be required by end users."
        )]
        pub type ToListenerMsg = crate::priv_client::transport::ToListenerMsg;

        #[deprecated(
            since = "0.15.0",
            note = "Reservation will become crate-private in the future
            as it shouldn't be required by end users."
        )]
        pub type Reservation = crate::priv_client::transport::Reservation;
    }
}

pub mod relay {
    #[deprecated(since = "0.15.0", note = "Use libp2p_relay::Config instead.")]
    pub type Config = crate::Config;

    #[deprecated(since = "0.15.0", note = "Use libp2p_relay::Event instead.")]
    pub type Event = crate::Event;

    #[deprecated(since = "0.15.0", note = "Use libp2p_relay::Behaviour instead.")]
    pub type Relay = crate::Behaviour;

    #[deprecated(since = "0.15.0", note = "Use libp2p_relay::CircuitId instead.")]
    pub type CircuitId = crate::CircuitId;

    pub mod rate_limiter {
        use instant::Instant;
        use libp2p_core::Multiaddr;
        use libp2p_identity::PeerId;

        #[deprecated(
            since = "0.15.0",
            note = "Use libp2p_relay::behaviour::rate_limiter::RateLimiter instead."
        )]
        pub trait RateLimiter: Send {
            fn try_next(&mut self, peer: PeerId, addr: &Multiaddr, now: Instant) -> bool;
        }

        #[allow(deprecated)]
        impl<T> RateLimiter for T
        where
            T: crate::behaviour::rate_limiter::RateLimiter,
        {
            fn try_next(&mut self, peer: PeerId, addr: &Multiaddr, now: Instant) -> bool {
                self.try_next(peer, addr, now)
            }
        }
    }
}

pub mod protocol {
    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::inbound::hop::FatalUpgradeError instead."
    )]
    pub type InboundHopFatalUpgradeError = crate::inbound::hop::FatalUpgradeError;

    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::inbound::stop::FatalUpgradeError instead."
    )]
    pub type InboundStopFatalUpgradeError = crate::inbound::stop::FatalUpgradeError;

    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::outbound::hop::FatalUpgradeError instead."
    )]
    pub type OutboundHopFatalUpgradeError = crate::outbound::hop::FatalUpgradeError;

    #[deprecated(
        since = "0.15.0",
        note = "Use libp2p_relay::outbound::stop::FatalUpgradeError instead."
    )]
    pub type OutboundStopFatalUpgradeError = crate::outbound::stop::FatalUpgradeError;
}

#[deprecated(
    since = "0.15.0",
    note = "RequestId will be deprecated as it isn't used"
)]
pub type RequestId = super::RequestId;
