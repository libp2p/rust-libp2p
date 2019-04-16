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
    ///   * The duration of a single ping timeout without sending or receiving
    ///     a pong is sufficient for the connection to be subject to being closed,
    ///     i.e. the connection timeout is reset to 20 seconds from the current
    ///     [`Instant`] upon a sent or received pong.
    ///
    /// In general, every successfully sent or received pong resets the connection
    /// timeout, which is defined by
    /// ```raw
    /// max_timeouts * timeout
    /// ```raw
    /// relative to the current [`Instant`].
    ///
    /// A sensible configuration should thus obey the invariant:
    /// ```raw
    /// max_timeouts * timeout > interval
    /// ```
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
    ///
    /// > **Note**: Successful inbound pings from the remote peer can keep
    /// >           the connection alive, even if outbound pings fail. I.e.
    /// >           the connection is closed after `ping_timeout * max_timeouts`
    /// >           only if in addition to failing outbound pings, no ping from
    /// >           the remote is received within that time window.
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
    /// This policy is appropriate e.g. if continuous measurement of
    /// the RTT to the remote is desired.
    Always,
    /// Only sent a ping as necessary to keep the connection alive.
    ///
    /// This policy resets the local ping timer whenever an inbound ping
    /// is received, effectively letting the peer with the lower ping
    /// frequency drive the ping protocol. Hence, to avoid superfluous ping
    /// traffic, the ping interval of the peers should ideally not be the
    /// same when using this policy, e.g. through randomization.
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

impl<TSubstream> PingHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
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
                self.connection_keepalive = KeepAlive::Until(instant + self.connection_timeout);
                let reset = match result {
                    Ok(PingSuccess::Ping { .. }) => true,
                    Ok(PingSuccess::Pong) => self.config.policy == PingPolicy::KeepAlive,
                    _ => false
                };
                if reset {
                    self.next_ping.reset(instant + self.config.interval);
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
