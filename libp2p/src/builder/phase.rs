#![allow(unused_imports)]

mod bandwidth_logging;
mod bandwidth_metrics;
mod behaviour;
mod build;
mod dns;
mod identity;
mod other_transport;
mod provider;
mod quic;
mod relay;
mod swarm;
mod tcp;
mod websocket;

use bandwidth_logging::*;
use bandwidth_metrics::*;
pub use behaviour::BehaviourError;
use behaviour::*;
use build::*;
use dns::*;
use libp2p_core::{muxing::StreamMuxerBox, Transport};
use libp2p_identity::Keypair;
pub use other_transport::TransportError;
use other_transport::*;
use provider::*;
use quic::*;
use relay::*;
use swarm::*;
use tcp::*;
#[cfg(all(not(target_arch = "wasm32"), feature = "websocket"))]
pub use websocket::WebsocketError;
use websocket::*;

use super::{
    select_muxer::SelectMuxerUpgrade, select_security::SelectSecurityUpgrade, SwarmBuilder,
};

#[allow(unreachable_pub)]
pub trait IntoSecurityUpgrade<C> {
    type Upgrade;
    type Error;

    fn into_security_upgrade(self, keypair: &Keypair) -> Result<Self::Upgrade, Self::Error>;
}

impl<C, T, F, E> IntoSecurityUpgrade<C> for F
where
    F: for<'a> FnOnce(&'a Keypair) -> Result<T, E>,
{
    type Upgrade = T;
    type Error = E;

    fn into_security_upgrade(self, keypair: &Keypair) -> Result<Self::Upgrade, Self::Error> {
        (self)(keypair)
    }
}

impl<F1, F2, C> IntoSecurityUpgrade<C> for (F1, F2)
where
    F1: IntoSecurityUpgrade<C>,
    F2: IntoSecurityUpgrade<C>,
{
    type Upgrade = SelectSecurityUpgrade<F1::Upgrade, F2::Upgrade>;
    type Error = either::Either<F1::Error, F2::Error>;

    fn into_security_upgrade(self, keypair: &Keypair) -> Result<Self::Upgrade, Self::Error> {
        let (f1, f2) = self;

        let u1 = f1
            .into_security_upgrade(keypair)
            .map_err(either::Either::Left)?;
        let u2 = f2
            .into_security_upgrade(keypair)
            .map_err(either::Either::Right)?;

        Ok(SelectSecurityUpgrade::new(u1, u2))
    }
}

#[allow(unreachable_pub)]
pub trait IntoMultiplexerUpgrade<C> {
    type Upgrade;

    fn into_multiplexer_upgrade(self) -> Self::Upgrade;
}

impl<C, U, F> IntoMultiplexerUpgrade<C> for F
where
    F: FnOnce() -> U,
{
    type Upgrade = U;

    fn into_multiplexer_upgrade(self) -> Self::Upgrade {
        (self)()
    }
}

impl<C, U1, U2> IntoMultiplexerUpgrade<C> for (U1, U2)
where
    U1: IntoMultiplexerUpgrade<C>,
    U2: IntoMultiplexerUpgrade<C>,
{
    type Upgrade = SelectMuxerUpgrade<U1::Upgrade, U2::Upgrade>;

    fn into_multiplexer_upgrade(self) -> Self::Upgrade {
        let (f1, f2) = self;

        let u1 = f1.into_multiplexer_upgrade();
        let u2 = f2.into_multiplexer_upgrade();

        SelectMuxerUpgrade::new(u1, u2)
    }
}

pub trait AuthenticatedMultiplexedTransport:
    Transport<
        Error = Self::E,
        Dial = Self::D,
        ListenerUpgrade = Self::U,
        Output = (libp2p_identity::PeerId, StreamMuxerBox),
    > + Send
    + Unpin
    + 'static
{
    type E: Send + Sync + 'static;
    type D: Send;
    type U: Send;
}

impl<T> AuthenticatedMultiplexedTransport for T
where
    T: Transport<Output = (libp2p_identity::PeerId, StreamMuxerBox)> + Send + Unpin + 'static,
    <T as Transport>::Error: Send + Sync + 'static,
    <T as Transport>::Dial: Send,
    <T as Transport>::ListenerUpgrade: Send,
{
    type E = T::Error;
    type D = T::Dial;
    type U = T::ListenerUpgrade;
}
