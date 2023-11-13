
use std::{collections::VecDeque, io, task::Poll, time::{Duration, Instant}};
use futures::{prelude::*, future::Either};
use libp2p::{PeerId, Stream, StreamProtocol, quic::Connection};
use libp2p_core::{Multiaddr, transport::MemoryTransport, Transport};
use rand::{distributions, prelude::*};
use futures_timer::Delay;

pub const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/ipfs/ping/1.0.0");

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
 
        peer
        let stream = self.connection.poll_next_inbound(cx)
            .transpose()
            .map_err(Error)?
            .map(Stream)
            .ok_or(Error(ConnectionError::Closed))?;

        todo!()

        // Stream::from(value);

        // Ok(())
    }
}

pub enum Error {
    NotConnected(PeerId),
    UnsupportedProtocol,
    Io(io::Error),
}

pub enum Event {
    /// A new stream is being opened
    OpenStream,

    /// An inbound stream failed
    InboundStreamFailed(io::Error),
}

/// A wrapper around send that enforces a time out.
async fn send_ping(stream: Stream, timeout: Duration) -> Result<(Stream, Duration), Error> {
    let ping = send(stream);
    futures::pin_mut!(ping);

    match future::select(ping, Delay::new(timeout)).await {
        Either::Left((Ok((stream, rtt)), _)) => Ok((stream, rtt)),
        Either::Left((Err(e), _)) =>todo!(),
        Either::Right(((), _)) => todo!(),
    }
}

pub async fn send<S>(mut stream: S) -> io::Result<(S, Duration)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let payload: [u8; 32] = thread_rng().sample(distributions::Standard);
    stream.write_all(&payload).await?;
    stream.flush().await?;
    let started = Instant::now();
    let mut recv_payload = [0u8; 32];
    stream.read_exact(&mut recv_payload).await?;
    if recv_payload == payload {
        Ok((stream, started.elapsed()))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Payload mismatch",
        ))
    }
}