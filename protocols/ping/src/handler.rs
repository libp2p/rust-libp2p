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
use futures::future::BoxFuture;
use libp2p_swarm::{
    KeepAlive,
    NegotiatedSubstream,
    SubstreamProtocol,
    ProtocolsHandler,
    ProtocolsHandlerUpgrErr,
    ProtocolsHandlerEvent
};
use std::{
    error::Error,
    io,
    fmt,
    num::NonZeroU32,
    task::{Context, Poll},
    time::Duration
};
use std::collections::VecDeque;
use wasm_timer::Delay;
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
    /// If the maximum number of allowed ping failures is reached, the
    /// connection is always terminated as a result of [`ProtocolsHandler::poll`]
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
pub struct PingHandler {
    /// Configuration options.
    config: PingConfig,
    /// The timer used for the delay to the next ping as well as
    /// the ping timeout.
    timer: Delay,
    /// Outbound ping failures that are pending to be processed by `poll()`.
    pending_errors: VecDeque<PingFailure>,
    /// The number of consecutive ping failures that occurred.
    ///
    /// Each successful ping resets this counter to 0.
    failures: u32,
    /// The outbound ping state.
    outbound: Option<PingState>,
    /// The inbound pong handler, i.e. if there is an inbound
    /// substream, this is always a future that waits for the
    /// next inbound ping to be answered.
    inbound: Option<PongFuture>,
}

impl PingHandler {
    /// Builds a new `PingHandler` with the given configuration.
    pub fn new(config: PingConfig) -> Self {
        PingHandler {
            config,
            timer: Delay::new(Duration::new(0, 0)),
            pending_errors: VecDeque::with_capacity(2),
            failures: 0,
            outbound: None,
            inbound: None,
        }
    }
}

impl ProtocolsHandler for PingHandler {
    type InEvent = Void;
    type OutEvent = PingResult;
    type Error = PingFailure;
    type InboundProtocol = protocol::Ping;
    type OutboundProtocol = protocol::Ping;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<protocol::Ping, ()> {
        SubstreamProtocol::new(protocol::Ping, ())
    }

    fn inject_fully_negotiated_inbound(&mut self, stream: NegotiatedSubstream, (): ()) {
        self.inbound = Some(protocol::recv_ping(stream).boxed());
    }

    fn inject_fully_negotiated_outbound(&mut self, stream: NegotiatedSubstream, (): ()) {
        self.timer.reset(self.config.timeout);
        self.outbound = Some(PingState::Ping(protocol::send_ping(stream).boxed()));
    }

    fn inject_event(&mut self, _: Void) {}

    fn inject_dial_upgrade_error(&mut self, _info: (), error: ProtocolsHandlerUpgrErr<Void>) {
        self.outbound = None; // Request a new substream on the next `poll`.
        self.pending_errors.push_front(
            match error {
                // Note: This timeout only covers protocol negotiation.
                ProtocolsHandlerUpgrErr::Timeout => PingFailure::Timeout,
                e => PingFailure::Other { error: Box::new(e) },
            })
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        if self.config.keep_alive {
            KeepAlive::Yes
        } else {
            KeepAlive::No
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<ProtocolsHandlerEvent<protocol::Ping, (), PingResult, Self::Error>> {
        // Respond to inbound pings.
        if let Some(fut) = self.inbound.as_mut() {
            match fut.poll_unpin(cx) {
                Poll::Pending => {},
                Poll::Ready(Err(e)) => {
                    log::debug!("Inbound ping error: {:?}", e);
                    self.inbound = None;
                }
                Poll::Ready(Ok(stream)) => {
                    // A ping from a remote peer has been answered, wait for the next.
                    self.inbound = Some(protocol::recv_ping(stream).boxed());
                    return Poll::Ready(ProtocolsHandlerEvent::Custom(Ok(PingSuccess::Pong)))
                }
            }
        }

        loop {
            // Check for outbound ping failures.
            if let Some(error) = self.pending_errors.pop_back() {
                log::debug!("Ping failure: {:?}", error);

                self.failures += 1;

                // Note: For backward-compatibility, with configured
                // `max_failures == 1`, the first failure is always "free"
                // and silent. This allows peers who still use a new substream
                // for each ping to have successful ping exchanges with peers
                // that use a single substream, since every successful ping
                // resets `failures` to `0`, while at the same time emitting
                // events only for `max_failures - 1` failures, as before.
                if self.failures > 1 || self.config.max_failures.get() > 1 {
                    if self.failures >= self.config.max_failures.get() {
                        log::debug!("Too many failures ({}). Closing connection.", self.failures);
                        return Poll::Ready(ProtocolsHandlerEvent::Close(error))
                    }

                    return Poll::Ready(ProtocolsHandlerEvent::Custom(Err(error)))
                }
            }

            // Continue outbound pings.
            match self.outbound.take() {
                Some(PingState::Ping(mut ping)) => match ping.poll_unpin(cx) {
                    Poll::Pending => {
                        if self.timer.poll_unpin(cx).is_ready() {
                            self.pending_errors.push_front(PingFailure::Timeout);
                        } else {
                            self.outbound = Some(PingState::Ping(ping));
                            break
                        }
                    },
                    Poll::Ready(Ok((stream, rtt))) => {
                        self.failures = 0;
                        self.timer.reset(self.config.interval);
                        self.outbound = Some(PingState::Idle(stream));
                        return Poll::Ready(
                            ProtocolsHandlerEvent::Custom(
                                Ok(PingSuccess::Ping { rtt })))
                    }
                    Poll::Ready(Err(e)) => {
                        self.pending_errors.push_front(PingFailure::Other {
                            error: Box::new(e)
                        });
                    }
                },
                Some(PingState::Idle(stream)) => match self.timer.poll_unpin(cx) {
                    Poll::Pending => {
                        self.outbound = Some(PingState::Idle(stream));
                        break
                    },
                    Poll::Ready(Ok(())) => {
                        self.timer.reset(self.config.timeout);
                        self.outbound = Some(PingState::Ping(protocol::send_ping(stream).boxed()));
                    },
                    Poll::Ready(Err(e)) => {
                        return Poll::Ready(ProtocolsHandlerEvent::Close(
                            PingFailure::Other {
                                error: Box::new(e)
                            }))
                    }
                }
                Some(PingState::OpenStream) => {
                    self.outbound = Some(PingState::OpenStream);
                    break
                }
                None => {
                    self.outbound = Some(PingState::OpenStream);
                    let protocol = SubstreamProtocol::new(protocol::Ping, ())
                        .with_timeout(self.config.timeout);
                    return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                        protocol
                    })
                }
            }
        }

        Poll::Pending
    }
}

type PingFuture = BoxFuture<'static, Result<(NegotiatedSubstream, Duration), io::Error>>;
type PongFuture = BoxFuture<'static, Result<NegotiatedSubstream, io::Error>>;

/// The current state w.r.t. outbound pings.
enum PingState {
    /// A new substream is being negotiated for the ping protocol.
    OpenStream,
    /// The substream is idle, waiting to send the next ping.
    Idle(NegotiatedSubstream),
    /// A ping is being sent and the response awaited.
    Ping(PingFuture),
}

