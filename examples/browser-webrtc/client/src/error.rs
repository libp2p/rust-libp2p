use libp2p::ping::Failure;
use libp2p::{multiaddr, swarm};

#[derive(Debug, thiserror::Error)]
pub(crate) enum PingerError {
    #[error("failed to ping node")]
    Ping(#[from] Failure),
    #[error("failed to parse address")]
    MultiaddrParse(#[from] multiaddr::Error),
    #[error("failed to dial node")]
    Dial(#[from] swarm::DialError),
}
