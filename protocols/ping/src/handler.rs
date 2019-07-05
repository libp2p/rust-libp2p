// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::protocol;
use futures::prelude::*;
use libp2p_swarm::{
    KeepAlive,
    SubstreamProtocol,
    ProtocolsHandler,
    ProtocolsHandlerUpgrErr,
    ProtocolsHandlerEvent
};
use std::{error::Error, io, fmt, num::NonZeroU32, time::Duration};
use std::collections::VecDeque;
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_timer::{Delay, Instant};
use void::Void;

/// The configuration for outbound pings.
#[derive(Clone, Debug)]
pub struct PingConfig {
    /// The timeout of an outbound ping.
    timeout: Duration,
    /// The duration between the last successful outbound or inbound ping
    /// and the next outbound ping.
    interval: Duration,
    /// The maximum number of failed outbound pings before the associated
    /// connection is deemed unhealthy, indicating to the `Swarm` that it
    /// should be closed.
    max_failures: NonZeroU32,
    /// Whether the connection should generally be kept alive unless
    /// `max_failures` occur.
    keep_alive: bool,
}

impl PingConfig {
    /// Creates a new `PingConfig` with the following default settings:
    ///
    ///   * [`PingConfig::with_interval`] 15s
    ///   * [`PingConfig::with_timeout`] 20s
    ///   * [`PingConfig::with_max_failures`] 1
    ///   * [`PingConfig::with_keep_alive`] false
    ///
    /// These settings have the following effect:
    ///
    ///   * A ping is sent every 15 seconds on a healthy connection.
    ///   * Every ping sent must yield a response within 20 seconds in order to
    ///     be successful.
    ///   * A single ping failure is sufficient for the connection to be subject
    ///     to being closed.
    ///   * The connection may be closed at any time as far as the ping protocol
    ///     is concerned, i.e. the ping protocol itself does not keep the
    ///     connection alive.
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(20),
            interval: Duration::from_secs(15),
            max_failures: NonZeroU32::new(1).expect("1 != 0"),
            keep_alive: false
        }
    }

    /// Sets the ping timeout.
    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    /// Sets the ping interval.
    pub fn with_interval(mut self, d: Duration) -> Self {
        self.interval = d;
        self
    }

    /// Sets the maximum number of consecutive ping failures upon which the remote
    /// peer is considered unreachable and the connection closed.
    pub fn with_max_failures(mut self, n: NonZeroU32) -> Self {
        self.max_failures = n;
        self
    }

    /// Sets whether the ping protocol itself should keep the connection alive,
    /// apart from the maximum allowed failures.
    ///
    /// By default, the ping protocol itself allows the connection to be closed
    /// at any time, i.e. in the absence of ping failures the connection lifetime
    /// is determined by other protocol handlers.
    ///
    /// If the maximum  number of allowed ping failures is reached, the
    /// connection is always terminated as a result of [`PingHandler::poll`]
    /// returning an error, regardless of the keep-alive setting.
    pub fn with_keep_alive(mut self, b: bool) -> Self {
        self.keep_alive = b;
        self
    }
}

/// The result of an inbound or outbound ping.
pub type PingResult = Result<PingSuccess, PingFailure>;

/// The successful result of processing an inbound or outbound ping.
#[derive(Debug)]
pub enum PingSuccess {
    /// Received a ping and sent back a pong.
    Pong,
    /// Sent a ping and received back a pong.
    ///
    /// Includes the round-trip time.
    Ping { rtt: Duration },
}

/// An outbound ping failure.
#[derive(Debug)]
pub enum PingFailure {
    /// The ping timed out, i.e. no response was received within the
    /// configured ping timeout.
    Timeout,
    /// The ping failed for reasons other than a timeout.
    Other { error: Box<dyn std::error::Error + Send + 'static> }
}

impl fmt::Display for PingFailure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PingFailure::Timeout => f.write_str("Ping timeout"),
            PingFailure::Other { error } => write!(f, "Ping error: {}", error)
        }
    }
}

impl Error for PingFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            PingFailure::Timeout => None,
            PingFailure::Other { error } => Some(&**error)
        }
    }
}

/// Protocol handler that handles pinging the remote at a regular period
/// and answering ping queries.
///
/// If the remote doesn't respond, produces an error that closes the connection.
pub struct PingHandler<TSubstream> {
    /// Configuration options.
    config: PingConfig,
    /// The timer for when to send the next ping.
    next_ping: Delay,
    /// The pending results from inbound or outbound pings, ready
    /// to be `poll()`ed.
    pending_results: VecDeque<PingResult>,
    /// The number of consecutive ping failures that occurred.
    failures: u32,
    _marker: std::marker::PhantomData<TSubstream>
}

impl<TSubstream> PingHandler<TSubstream> {
    /// Builds a new `PingHandler` with the given configuration.
    pub fn new(config: PingConfig) -> Self {
        PingHandler {
            config,
            next_ping: Delay::new(Instant::now()),
            pending_results: VecDeque::with_capacity(2),
            failures: 0,
            _marker: std::marker::PhantomData
        }
    }
}

impl<TSubstream> ProtocolsHandler for PingHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = Void;
    type OutEvent = PingResult;
    type Error = PingFailure;
    type Substream = TSubstream;
    type InboundProtocol = protocol::Ping;
    type OutboundProtocol = protocol::Ping;
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<protocol::Ping> {
        SubstreamProtocol::new(protocol::Ping)
    }

    fn inject_fully_negotiated_inbound(&mut self, _: ()) {
        // A ping from a remote peer has been answered.
        self.pending_results.push_front(Ok(PingSuccess::Pong));
    }

    fn inject_fully_negotiated_outbound(&mut self, rtt: Duration, _info: ()) {
        // A ping initiated by the local peer was answered by the remote.
        self.pending_results.push_front(Ok(PingSuccess::Ping { rtt }));
    }

    fn inject_event(&mut self, _: Void) {}

    fn inject_dial_upgrade_error(&mut self, _info: (), error: ProtocolsHandlerUpgrErr<io::Error>) {
        self.pending_results.push_front(
            Err(match error {
                ProtocolsHandlerUpgrErr::Timeout => PingFailure::Timeout,
                e => PingFailure::Other { error: Box::new(e) }
            }))
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        if self.config.keep_alive {
            KeepAlive::Yes
        } else {
            KeepAlive::No
        }
    }

    fn poll(&mut self) -> Poll<ProtocolsHandlerEvent<protocol::Ping, (), PingResult>, Self::Error> {
        if let Some(result) = self.pending_results.pop_back() {
            if let Ok(PingSuccess::Ping { .. }) = result {
                let next_ping = Instant::now() + self.config.interval;
                self.failures = 0;
                self.next_ping.reset(next_ping);
            }
            if let Err(e) = result {
                self.failures += 1;
                if self.failures >= self.config.max_failures.get() {
                    return Err(e)
                } else {
                    return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(Err(e))))
                }
            }
            return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(result)))
        }

        match self.next_ping.poll() {
            Ok(Async::Ready(())) => {
                self.next_ping.reset(Instant::now() + self.config.timeout);
                let protocol = SubstreamProtocol::new(protocol::Ping)
                    .with_timeout(self.config.timeout);
                Ok(Async::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    protocol,
                    info: (),
                }))
            },
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(e) => Err(PingFailure::Other { error: Box::new(e) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::future;
    use quickcheck::*;
    use rand::Rng;
    use tokio_tcp::TcpStream;
    use tokio::runtime::current_thread::Runtime;

    impl Arbitrary for PingConfig {
        fn arbitrary<G: Gen>(g: &mut G) -> PingConfig {
            PingConfig::new()
                .with_timeout(Duration::from_secs(g.gen_range(0, 3600)))
                .with_interval(Duration::from_secs(g.gen_range(0, 3600)))
                .with_max_failures(NonZeroU32::new(g.gen_range(1, 100)).unwrap())
        }
    }

    fn tick(h: &mut PingHandler<TcpStream>) -> Result<
        ProtocolsHandlerEvent<protocol::Ping, (), PingResult>,
        PingFailure
    > {
        Runtime::new().unwrap().block_on(future::poll_fn(|| h.poll() ))
    }

    #[test]
    fn ping_interval() {
        fn prop(cfg: PingConfig, ping_rtt: Duration) -> bool {
            let mut h = PingHandler::<TcpStream>::new(cfg);

            // The first ping is scheduled "immediately".
            let start = h.next_ping.deadline();
            assert!(start <= Instant::now());

            // Send ping
            match tick(&mut h) {
                Ok(ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info: _ }) => {
                    // The handler must use the configured timeout.
                    assert_eq!(protocol.timeout(), &h.config.timeout);
                    // The next ping must be scheduled no earlier than the ping timeout.
                    assert!(h.next_ping.deadline() >= start + h.config.timeout);
                }
                e => panic!("Unexpected event: {:?}", e)
            }

            let now = Instant::now();

            // Receive pong
            h.inject_fully_negotiated_outbound(ping_rtt, ());
            match tick(&mut h) {
                Ok(ProtocolsHandlerEvent::Custom(Ok(PingSuccess::Ping { rtt }))) => {
                    // The handler must report the given RTT.
                    assert_eq!(rtt, ping_rtt);
                    // The next ping must be scheduled no earlier than the ping interval.
                    assert!(now + h.config.interval <= h.next_ping.deadline());
                }
                e => panic!("Unexpected event: {:?}", e)
            }
            true
        }

        quickcheck(prop as fn(_,_) -> _);
    }

    #[test]
    fn max_failures() {
        let cfg = PingConfig::arbitrary(&mut StdGen::new(rand::thread_rng(), 100));
        let mut h = PingHandler::<TcpStream>::new(cfg);
        for _ in 0 .. h.config.max_failures.get() - 1 {
            h.inject_dial_upgrade_error((), ProtocolsHandlerUpgrErr::Timeout);
            match tick(&mut h) {
                Ok(ProtocolsHandlerEvent::Custom(Err(PingFailure::Timeout))) => {}
                e => panic!("Unexpected event: {:?}", e)
            }
        }
        h.inject_dial_upgrade_error((), ProtocolsHandlerUpgrErr::Timeout);
        match tick(&mut h) {
            Err(PingFailure::Timeout) => {
                assert_eq!(h.failures, h.config.max_failures.get());
            }
            e => panic!("Unexpected event: {:?}", e)
        }
        h.inject_fully_negotiated_outbound(Duration::from_secs(1), ());
        match tick(&mut h) {
            Ok(ProtocolsHandlerEvent::Custom(Ok(PingSuccess::Ping { .. }))) => {
                // A success resets the counter for consecutive failures.
                assert_eq!(h.failures, 0);
            }
            e => panic!("Unexpected event: {:?}", e)
        }
    }
}
