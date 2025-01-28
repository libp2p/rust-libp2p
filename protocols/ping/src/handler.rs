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

use std::{
    collections::VecDeque,
    convert::Infallible,
    error::Error,
    fmt, io,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::{BoxFuture, Either},
    prelude::*,
};
use futures_timer::Delay;
use libp2p_core::upgrade::ReadyUpgrade;
use libp2p_swarm::{
    handler::{ConnectionEvent, DialUpgradeError, FullyNegotiatedInbound, FullyNegotiatedOutbound},
    ConnectionHandler, ConnectionHandlerEvent, Stream, StreamProtocol, StreamUpgradeError,
    SubstreamProtocol,
};

use crate::{protocol, PROTOCOL_NAME};

/// The configuration for outbound pings.
#[derive(Debug, Clone)]
pub struct Config {
    /// The timeout of an outbound ping.
    timeout: Duration,
    /// The duration between outbound pings.
    interval: Duration,
}

impl Config {
    /// Creates a new [`Config`] with the following default settings:
    ///
    ///   * [`Config::with_interval`] 15s
    ///   * [`Config::with_timeout`] 20s
    ///
    /// These settings have the following effect:
    ///
    ///   * A ping is sent every 15 seconds on a healthy connection.
    ///   * Every ping sent must yield a response within 20 seconds in order to be successful.
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(20),
            interval: Duration::from_secs(15),
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
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

/// An outbound ping failure.
#[derive(Debug)]
pub enum Failure {
    /// The ping timed out, i.e. no response was received within the
    /// configured ping timeout.
    Timeout,
    /// The peer does not support the ping protocol.
    Unsupported,
    /// The ping failed for reasons other than a timeout.
    Other {
        error: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

impl Failure {
    fn other(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Other { error: Box::new(e) }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Timeout => f.write_str("Ping timeout"),
            Failure::Other { error } => write!(f, "Ping error: {error}"),
            Failure::Unsupported => write!(f, "Ping protocol not supported"),
        }
    }
}

impl Error for Failure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Failure::Timeout => None,
            Failure::Other { error } => Some(&**error),
            Failure::Unsupported => None,
        }
    }
}

/// Protocol handler that handles pinging the remote at a regular period
/// and answering ping queries.
pub struct Handler {
    /// Configuration options.
    config: Config,
    /// The timer used for the delay to the next ping.
    interval: Delay,
    /// Outbound ping failures that are pending to be processed by `poll()`.
    pending_errors: VecDeque<Failure>,
    /// The number of consecutive ping failures that occurred.
    ///
    /// Each successful ping resets this counter to 0.
    failures: u32,
    /// The outbound ping state.
    outbound: Option<OutboundState>,
    /// The inbound pong handler, i.e. if there is an inbound
    /// substream, this is always a future that waits for the
    /// next inbound ping to be answered.
    inbound: Option<PongFuture>,
    /// Tracks the state of our handler.
    state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// We are inactive because the other peer doesn't support ping.
    Inactive {
        /// Whether or not we've reported the missing support yet.
        ///
        /// This is used to avoid repeated events being emitted for a specific connection.
        reported: bool,
    },
    /// We are actively pinging the other peer.
    Active,
}

impl Handler {
    /// Builds a new [`Handler`] with the given configuration.
    pub fn new(config: Config) -> Self {
        Handler {
            config,
            interval: Delay::new(Duration::new(0, 0)),
            pending_errors: VecDeque::with_capacity(2),
            failures: 0,
            outbound: None,
            inbound: None,
            state: State::Active,
        }
    }

    fn on_dial_upgrade_error(
        &mut self,
        DialUpgradeError { error, .. }: DialUpgradeError<
            (),
            <Self as ConnectionHandler>::OutboundProtocol,
        >,
    ) {
        self.outbound = None; // Request a new substream on the next `poll`.

        // Timer is already polled and expired before substream request is initiated
        // and will be polled again later on in our `poll` because we reset `self.outbound`.
        //
        // `futures-timer` allows an expired timer to be polled again and returns
        // immediately `Poll::Ready`. However in its WASM implementation there is
        // a bug that causes the expired timer to panic.
        // This is a workaround until a proper fix is merged and released.
        // See libp2p/rust-libp2p#5447 for more info.
        //
        // TODO: remove when async-rs/futures-timer#74 gets merged.
        self.interval.reset(Duration::new(0, 0));

        let error = match error {
            StreamUpgradeError::NegotiationFailed => {
                debug_assert_eq!(self.state, State::Active);

                self.state = State::Inactive { reported: false };
                return;
            }
            // Note: This timeout only covers protocol negotiation.
            StreamUpgradeError::Timeout => Failure::Other {
                error: Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "ping protocol negotiation timed out",
                )),
            },
            // TODO: remove when Rust 1.82 is MSRV
            #[allow(unreachable_patterns)]
            StreamUpgradeError::Apply(e) => libp2p_core::util::unreachable(e),
            StreamUpgradeError::Io(e) => Failure::Other { error: Box::new(e) },
        };

        self.pending_errors.push_front(error);
    }
}

impl ConnectionHandler for Handler {
    type FromBehaviour = Infallible;
    type ToBehaviour = Result<Duration, Failure>;
    type InboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundProtocol = ReadyUpgrade<StreamProtocol>;
    type OutboundOpenInfo = ();
    type InboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<ReadyUpgrade<StreamProtocol>> {
        SubstreamProtocol::new(ReadyUpgrade::new(PROTOCOL_NAME), ())
    }

    fn on_behaviour_event(&mut self, _: Infallible) {}

    #[tracing::instrument(level = "trace", name = "ConnectionHandler::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<ReadyUpgrade<StreamProtocol>, (), Result<Duration, Failure>>>
    {
        match self.state {
            State::Inactive { reported: true } => {
                return Poll::Pending; // nothing to do on this connection
            }
            State::Inactive { reported: false } => {
                self.state = State::Inactive { reported: true };
                return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Err(
                    Failure::Unsupported,
                )));
            }
            State::Active => {}
        }

        // Respond to inbound pings.
        if let Some(fut) = self.inbound.as_mut() {
            match fut.poll_unpin(cx) {
                Poll::Pending => {}
                Poll::Ready(Err(e)) => {
                    tracing::debug!("Inbound ping error: {:?}", e);
                    self.inbound = None;
                }
                Poll::Ready(Ok(stream)) => {
                    tracing::trace!("answered inbound ping from peer");

                    // A ping from a remote peer has been answered, wait for the next.
                    self.inbound = Some(protocol::recv_ping(stream).boxed());
                }
            }
        }

        loop {
            // Check for outbound ping failures.
            if let Some(error) = self.pending_errors.pop_back() {
                tracing::debug!("Ping failure: {:?}", error);

                self.failures += 1;

                // Note: For backward-compatibility the first failure is always "free"
                // and silent. This allows peers who use a new substream
                // for each ping to have successful ping exchanges with peers
                // that use a single substream, since every successful ping
                // resets `failures` to `0`.
                if self.failures > 1 {
                    return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Err(error)));
                }
            }

            // Continue outbound pings.
            match self.outbound.take() {
                Some(OutboundState::Ping(mut ping)) => match ping.poll_unpin(cx) {
                    Poll::Pending => {
                        self.outbound = Some(OutboundState::Ping(ping));
                        break;
                    }
                    Poll::Ready(Ok((stream, rtt))) => {
                        tracing::debug!(?rtt, "ping succeeded");
                        self.failures = 0;
                        self.interval.reset(self.config.interval);
                        self.outbound = Some(OutboundState::Idle(stream));
                        return Poll::Ready(ConnectionHandlerEvent::NotifyBehaviour(Ok(rtt)));
                    }
                    Poll::Ready(Err(e)) => {
                        self.interval.reset(self.config.interval);
                        self.pending_errors.push_front(e);
                    }
                },
                Some(OutboundState::Idle(stream)) => match self.interval.poll_unpin(cx) {
                    Poll::Pending => {
                        self.outbound = Some(OutboundState::Idle(stream));
                        break;
                    }
                    Poll::Ready(()) => {
                        self.outbound = Some(OutboundState::Ping(
                            send_ping(stream, self.config.timeout).boxed(),
                        ));
                    }
                },
                Some(OutboundState::OpenStream) => {
                    self.outbound = Some(OutboundState::OpenStream);
                    break;
                }
                None => match self.interval.poll_unpin(cx) {
                    Poll::Pending => break,
                    Poll::Ready(()) => {
                        self.outbound = Some(OutboundState::OpenStream);
                        let protocol = SubstreamProtocol::new(ReadyUpgrade::new(PROTOCOL_NAME), ());
                        return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                            protocol,
                        });
                    }
                },
            }
        }

        Poll::Pending
    }

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(FullyNegotiatedInbound {
                protocol: mut stream,
                ..
            }) => {
                stream.ignore_for_keep_alive();
                self.inbound = Some(protocol::recv_ping(stream).boxed());
            }
            ConnectionEvent::FullyNegotiatedOutbound(FullyNegotiatedOutbound {
                protocol: mut stream,
                ..
            }) => {
                stream.ignore_for_keep_alive();
                self.outbound = Some(OutboundState::Ping(
                    send_ping(stream, self.config.timeout).boxed(),
                ));
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                self.on_dial_upgrade_error(dial_upgrade_error)
            }
            _ => {}
        }
    }
}

type PingFuture = BoxFuture<'static, Result<(Stream, Duration), Failure>>;
type PongFuture = BoxFuture<'static, Result<Stream, io::Error>>;

/// The current state w.r.t. outbound pings.
enum OutboundState {
    /// A new substream is being negotiated for the ping protocol.
    OpenStream,
    /// The substream is idle, waiting to send the next ping.
    Idle(Stream),
    /// A ping is being sent and the response awaited.
    Ping(PingFuture),
}

/// A wrapper around [`protocol::send_ping`] that enforces a time out.
async fn send_ping(stream: Stream, timeout: Duration) -> Result<(Stream, Duration), Failure> {
    let ping = protocol::send_ping(stream);
    futures::pin_mut!(ping);

    match future::select(ping, Delay::new(timeout)).await {
        Either::Left((Ok((stream, rtt)), _)) => Ok((stream, rtt)),
        Either::Left((Err(e), _)) => Err(Failure::other(e)),
        Either::Right(((), _)) => Err(Failure::Timeout),
    }
}
