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
use libp2p_core::ProtocolsHandlerEvent;
use libp2p_core::protocols_handler::{
    KeepAlive,
    SubstreamProtocol,
    ProtocolsHandler,
    ProtocolsHandlerUpgrErr,
};
use std::{io, num::NonZeroU32, time::{Duration, Instant}};
use std::collections::VecDeque;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Delay;
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
    max_timeouts: NonZeroU32,
    /// The policy for outbound pings.
    policy: PingPolicy
}

impl PingConfig {
    /// Creates a new `PingConfig` with the following default settings:
    ///
    ///   * [`PingConfig::with_interval`] 15s
    ///   * [`PingConfig::with_timeout`] 20s
    ///   * [`PingConfig::with_max_timeouts`] 1
    ///   * [`PingConfig::with_policy`] [`PingPolicy::Always`]
    ///
    /// These settings have the following effect:
    ///
    ///   * A ping is sent every 15 seconds on a healthy connection.
    ///   * Every ping sent must yield a response within 20 seconds in order to
    ///     be successful.
    ///   * A single ping timeout is sufficient for the connection to be subject
    ///     to being closed, i.e. the connection timeout is reset to 20 seconds
    ///     beyond the instant of the next ping upon receiving a pong.
    ///
    /// In general, the connection timeout is defined by
    /// ```raw
    /// max_timeouts * timeout
    /// ```
    /// and the countdown begins from the instant of the next outbound ping.
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(20),
            interval: Duration::from_secs(15),
            max_timeouts: NonZeroU32::new(1).expect("1 != 0"),
            policy: PingPolicy::Always
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

    /// Sets the number of successive ping timeouts upon which the remote
    /// peer is considered unreachable and the connection closed.
    pub fn with_max_timeouts(mut self, n: NonZeroU32) -> Self {
        self.max_timeouts = n;
        self
    }

    /// Sets the [`PingPolicy`] to use for outbound pings.
    pub fn with_policy(mut self, p: PingPolicy) -> Self {
        self.policy = p;
        self
    }
}

/// The `PingPolicy` determines under what conditions an outbound ping
/// is sent w.r.t. inbound pings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PingPolicy {
    /// Always send a ping in the configured interval, regardless
    /// of received pings.
    ///
    /// The connection [`KeepAlive`] is renewed on every received pong.
    ///
    /// This policy is appropriate e.g. if continuous measurement of
    /// the RTT to the remote is desired.
    Always,
    /// Only sent a ping as necessary to keep the connection alive.
    ///
    /// This policy also resets the local ping timer and renews the connection
    /// [`KeepAlive`] whenever a pong is sent, effectively letting the peer
    /// with the lower ping frequency drive the ping protocol. Hence, to avoid
    /// superfluous ping protocol traffic, the ping interval of the peers should
    /// ideally not be the same when using this policy, e.g. through randomization.
    ///
    /// This policy is appropriate if the ping protocol is only used
    /// as an application-layer connection keep-alive, without a need
    /// for measuring the round-trip times on both peers, as it tries
    /// to keep the ping traffic to a minimum.
    KeepAlive
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

/// Protocol handler that handles pinging the remote at a regular period
/// and answering ping queries.
///
/// If the remote doesn't respond, produces an error that closes the connection.
pub struct PingHandler<TSubstream> {
    /// Configuration options.
    config: PingConfig,
    /// The timer for when to send the next ping.
    next_ping: Delay,
    /// The connection timeout.
    connection_timeout: Duration,
    /// The current keep-alive, i.e. until when the connection that
    /// the handler operates on should be kept alive.
    connection_keepalive: KeepAlive,
    /// The pending results from inbound or outbound pings, ready
    /// to be `poll()`ed.
    pending_results: VecDeque<(Instant, PingResult)>,
    _marker: std::marker::PhantomData<TSubstream>
}

impl<TSubstream> PingHandler<TSubstream> {
    /// Builds a new `PingHandler` with the given configuration.
    pub fn new(config: PingConfig) -> Self {
        let now = Instant::now();
        let connection_timeout = config.timeout * config.max_timeouts.get();
        let connection_keepalive = KeepAlive::Until(now + connection_timeout);
        let next_ping = Delay::new(now);
        PingHandler {
            config,
            next_ping,
            connection_timeout,
            connection_keepalive,
            pending_results: VecDeque::with_capacity(2),
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
    type Error = ProtocolsHandlerUpgrErr<io::Error>;
    type Substream = TSubstream;
    type InboundProtocol = protocol::Ping;
    type OutboundProtocol = protocol::Ping;
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<protocol::Ping> {
        SubstreamProtocol::new(protocol::Ping)
    }

    fn inject_fully_negotiated_inbound(&mut self, _: ()) {
        // A ping from a remote peer has been answered.
        self.pending_results.push_front((Instant::now(), Ok(PingSuccess::Pong)));
    }

    fn inject_fully_negotiated_outbound(&mut self, rtt: Duration, _info: ()) {
        // A ping initiated by the local peer was answered by the remote.
        self.pending_results.push_front((Instant::now(), Ok(PingSuccess::Ping { rtt })));
    }

    fn inject_event(&mut self, _: Void) {}

    fn inject_dial_upgrade_error(&mut self, _info: (), error: Self::Error) {
        self.pending_results.push_front(
            (Instant::now(), Err(match error {
                ProtocolsHandlerUpgrErr::Timeout => PingFailure::Timeout,
                e => PingFailure::Other { error: Box::new(e) }
            })))
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.connection_keepalive
    }

    fn poll(&mut self) -> Poll<ProtocolsHandlerEvent<protocol::Ping, (), PingResult>, Self::Error> {
        if let Some((instant, result)) = self.pending_results.pop_back() {
            if result.is_ok() {
                let keepalive = match result {
                    Ok(PingSuccess::Ping { .. }) => true,
                    Ok(PingSuccess::Pong) => self.config.policy == PingPolicy::KeepAlive,
                    _ => false
                };
                if keepalive {
                    let next_ping = instant + self.config.interval;
                    let keep_until = next_ping + self.connection_timeout;
                    self.connection_keepalive = KeepAlive::Until(keep_until);
                    self.next_ping.reset(next_ping);
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
            Err(_) => Err(ProtocolsHandlerUpgrErr::Timer)
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
                .with_max_timeouts(NonZeroU32::new(g.gen_range(1, 100)).unwrap())
                .with_policy(if g.gen() { PingPolicy::Always } else { PingPolicy::KeepAlive })
        }
    }

    fn tick(h: &mut PingHandler<TcpStream>) -> ProtocolsHandlerEvent<protocol::Ping, (), PingResult> {
        Runtime::new().unwrap().block_on(future::poll_fn(|| h.poll() )).unwrap()
    }

    fn check_connection_keepalive(now: Instant, h: &PingHandler<TcpStream>) {
        match h.connection_keepalive {
            KeepAlive::Until(t) => {
                let next_ping = h.next_ping.deadline();
                // The next ping must be scheduled no earlier than the ping interval
                // and no later than the connection timeout.
                // nb. next_ping == t iff timeout == interval == 0
                assert!(now + h.config.interval <= next_ping && next_ping <= t);
                // The "countdown" for the connection timeout starts on the next ping.
                assert!(t - next_ping == h.connection_timeout)
            }
            k => panic!("Unexpected connection keepalive: {:?}", k)
        }
    }

    #[test]
    fn connection_timeout() {
        fn prop(cfg: PingConfig, ping_rtt: Duration) -> bool {
            let mut h = PingHandler::<TcpStream>::new(cfg);

            // The first ping is scheduled "immediately".
            let start = h.next_ping.deadline();
            assert!(start <= Instant::now());

            // Send ping
            match tick(&mut h) {
                ProtocolsHandlerEvent::OutboundSubstreamRequest { protocol, info: _ } => {
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
                ProtocolsHandlerEvent::Custom(Ok(PingSuccess::Ping { rtt })) => {
                    // The handler must report the given RTT.
                    assert_eq!(rtt, ping_rtt);
                    check_connection_keepalive(now, &h);
                }
                e => panic!("Unexpected event: {:?}", e)
            }
            true
        }

        quickcheck(prop as fn(_,_) -> _);
    }

    #[test]
    fn report_timeout() {
        let cfg = PingConfig::arbitrary(&mut StdGen::new(rand::thread_rng(), 100));
        let mut h = PingHandler::<TcpStream>::new(cfg);
        h.inject_dial_upgrade_error((), ProtocolsHandlerUpgrErr::Timeout);
        match tick(&mut h) {
            ProtocolsHandlerEvent::Custom(Err(PingFailure::Timeout)) => {}
            e => panic!("Unexpected event: {:?}", e)
        }
    }

    #[test]
    fn policy_keepalive_inbound() {
        let cfg = PingConfig::arbitrary(&mut StdGen::new(rand::thread_rng(), 100))
            .with_policy(PingPolicy::KeepAlive);
        let mut h = PingHandler::<TcpStream>::new(cfg);
        let now = Instant::now();
        h.inject_fully_negotiated_inbound(());
        match tick(&mut h) {
            ProtocolsHandlerEvent::Custom(Ok(PingSuccess::Pong)) => {
                check_connection_keepalive(now, &h);
            }
            e => panic!("Unexpected event: {:?}", e)
        }
    }
}
