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
    ProtocolsHandler,
    ProtocolsHandlerUpgrErr,
};
use std::{io, num::NonZeroU32, time::{Duration, Instant}};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Delay;
use void::Void;

/// The configuration for outbound pings.
#[derive(Clone)]
pub struct PingConfig {
    /// The timeout of an outbound ping.
    timeout: Duration,
    /// The duration between the last successful outbound or inbound ping
    /// and the next outbound ping.
    interval: Duration,
    /// The maximum number of failed outbound pings before the associated
    /// connection is deemed unhealthy, indicating to the `Swarm` that it
    /// should be closed.
    max_timeouts: NonZeroU32
}

impl PingConfig {
    /// Creates a new `PingConfig` with the following default settings:
    ///
    ///   * [`PingConfig::with_interval`] 20s
    ///   * [`PingConfig::with_timeout`] 10s
    ///   * [`PingConfig::with_max_timeouts`] 3
    ///
    /// These settings have the following effect:
    ///
    ///   * A ping is sent every 20 seconds on a healthy connection
    ///     (if no ping is received meanwhile).
    ///   * Every ping sent must yield a response within 10 seconds.
    ///   * If 3 subsequent outbound pings fail and no ping is received, i.e.
    ///     no ping is successfully sent or received within 30 seconds, the
    ///     connection is deemed unhealthy.
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            interval: Duration::from_secs(20),
            max_timeouts: NonZeroU32::new(3).expect("3 != 0")
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
    /// >           only if in addition to failing outbound pings no ping from
    /// >           the remote is received within that time window.
    pub fn with_max_timeouts(mut self, n: NonZeroU32) -> Self {
        self.max_timeouts = n;
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
    /// The last result from an inbound or outbound ping.
    last_result: Option<PingResult>,
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
            last_result: None,
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

    /// The outbound ping timeout, as specified in the [`PingConfig`].
    fn outbound_timeout(&self) -> Duration {
        self.config.timeout
    }

    fn listen_protocol(&self) -> protocol::Ping {
        protocol::Ping
    }

    fn inject_fully_negotiated_inbound(&mut self, _: ()) {
        // A ping from a remote peer has been answered.
        self.last_result = Some(Ok(PingSuccess::Pong));
    }

    fn inject_fully_negotiated_outbound(&mut self, rtt: Duration, _info: ()) {
        // A ping initiated by the local peer was answered by the remote.
        self.last_result = Some(Ok(PingSuccess::Ping { rtt }));
    }

    fn inject_event(&mut self, _: Void) {}

    fn inject_dial_upgrade_error(&mut self, _info: (), error: Self::Error) {
        self.last_result = Some(Err(match error {
            ProtocolsHandlerUpgrErr::Timeout => PingFailure::Timeout,
            e => PingFailure::Other { error: Box::new(e) }
        }))
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.connection_keepalive
    }

    fn poll(&mut self) -> Poll<ProtocolsHandlerEvent<protocol::Ping, (), PingResult>, Self::Error> {
        if let Some(res) = self.last_result.take() {
            if res.is_ok() {
                let now = Instant::now();
                self.connection_keepalive = KeepAlive::Until(now + self.connection_timeout);
                self.next_ping.reset(now + self.config.interval);
            }
            return Ok(Async::Ready(ProtocolsHandlerEvent::Custom(res)))
        }

        match self.next_ping.poll() {
            Ok(Async::Ready(())) => {
                self.next_ping.reset(Instant::now() + self.config.timeout);
                Ok(Async::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                    upgrade: protocol::Ping,
                    info: (),
                }))
            },
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(_) => Err(ProtocolsHandlerUpgrErr::Timer)
        }
    }
}
