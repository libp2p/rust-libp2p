
use std::{collections::VecDeque, io, task::Poll, time::{Duration, Instant}};
use futures::{prelude::*, future::Either};
use libp2p::{PeerId, Stream, StreamProtocol, quic::Connection};
use libp2p_core::{Multiaddr, transport::MemoryTransport, Transport};
use rand::{distributions, prelude::*};
use futures_timer::Delay;

pub const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/ipfs/ping/1.0.0");


pub enum Error {
	NotConnected(PeerId),
	UnsupportedProtocol,
	Io(io::Error)
}

pub enum Event {
	InboundStreamFailed(io::Error),
}

/// A behaviour that manages requests to open new streams which are directly handed to the user.
pub struct Behaviour {
    /// Config
    config: Config,
    /// Queue of events to yield to the swarm.
    events: VecDeque<Event>,
    /// Protocol
    protocol: StreamProtocol,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// The timeout of an outbound ping.
    timeout: Duration,
    /// The duration between outbound pings.
    interval: Duration,
}

pub struct IncomingStreams {
    /// Queue of events to yield to the swarm.
    events: VecDeque<Event>,
}

/// A control acts as a "remote" that allows users to request streams without interacting with the `Swarm` directly.
#[derive(Clone)]
pub struct Control {
}

impl IncomingStreams {
    pub async fn next(&mut self) -> (PeerId, Stream) {
        if let Some(e) = self.events.pop_back() {
        } else {
        }

        todo!()
    }

    pub fn poll_next(&mut self) -> Poll<(PeerId, Stream)> {
        if let Some(e) = self.events.pop_back() {
        } else {
        }

        todo!()
    }
}

impl Behaviour {
    pub fn new(protocol: StreamProtocol) -> (Self, Control, IncomingStreams) {
        (
            Self {
                config: Config {
                    timeout: Duration::from_secs(1),
                    interval: Duration::from_secs(1),
                },
                events: VecDeque::new(),
                protocol,
            },
            Control {},
            IncomingStreams {
                events: VecDeque::new()
            },
        )
    }
}

impl Control {
    pub async fn open_stream(&self, peer: PeerId) -> Result<Stream, Error> {
        todo!()
    }
}
