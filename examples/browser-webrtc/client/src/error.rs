use libp2p::ping::Failure;
use libp2p::{multiaddr, swarm};

#[derive(Debug)]
pub(crate) enum PingerError {
    Ping(Failure),
    MultiaddrParse(multiaddr::Error),
    Dial(swarm::DialError),
}

impl From<Failure> for PingerError {
    fn from(f: Failure) -> Self {
        PingerError::Ping(f)
    }
}

impl From<multiaddr::Error> for PingerError {
    fn from(err: multiaddr::Error) -> Self {
        PingerError::MultiaddrParse(err)
    }
}

impl From<swarm::DialError> for PingerError {
    fn from(err: swarm::DialError) -> Self {
        PingerError::Dial(err)
    }
}
