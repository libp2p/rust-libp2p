use futures::channel;
use libp2p::ping::Failure;
use libp2p::{multiaddr, swarm};

#[derive(Debug)]
pub(crate) enum PingerError {
    Ping(Failure),
    MultiaddrParse(multiaddr::Error),
    Dial(swarm::DialError),
    Other(String),
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

impl From<channel::mpsc::SendError> for PingerError {
    fn from(err: channel::mpsc::SendError) -> Self {
        PingerError::Other(format!("SendError: {:?}", err))
    }
}
