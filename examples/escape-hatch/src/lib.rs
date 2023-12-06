
use std::{collections::VecDeque, io, task::Poll, time::{Duration, Instant}, pin::Pin};
use futures::{prelude::*, future::Either, ready};
use libp2p::{PeerId, Stream, StreamProtocol, quic::Connection, noise, yamux, identity, tcp::tokio::TcpStream};
use libp2p_core::{Multiaddr, transport::{MemoryTransport, ListenerId}, Transport, upgrade};
use rand::{distributions, prelude::*};
use futures_timer::Delay;
use std::task::{Context};
use futures::future::poll_fn;
use tokio::{ io::AsyncWrite, net::tcp};
use libp2p_core::{
    multiaddr::multiaddr,
}

// TODO remove unneeded dependencies

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
    stream: Option<TcpStream>,
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

    pub fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<(PeerId, Stream)> {

        let ping_message = "Ping\n";

        // TODO remove unwrap
        let stream_ref = self.stream.as_mut().unwrap();

        if let Some(e) = self.events.pop_back() {
            let result = match Pin::new(stream_ref).poll_write(cx, ping_message.as_bytes()) {
                Poll::Ready(Ok(_)) => Poll::Pending,
                Poll::Ready(Err(e)) => Poll::Ready(()),
                Poll::Pending => Poll::Pending,
            };
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
                stream: None,
                events: VecDeque::new()
            },
        )
    }
}

impl Control {
    pub async fn open_stream(&self, peer: PeerId) -> Result<TcpStream, Error> {    
        let stream =  tokio::net::TcpStream::connect("/ip4/0.0.0.0/tcp/0").await.unwrap();
        Ok( TcpStream(stream))
    }
}
