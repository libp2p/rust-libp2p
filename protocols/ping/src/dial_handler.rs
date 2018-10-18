// Copyright 2018 Parity Technologies (UK) Ltd.
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

use futures::prelude::*;
use libp2p_core::{
    nodes::{NodeHandlerEndpoint, ProtocolsHandler, ProtocolsHandlerEvent},
    upgrade::toggleable,
    ConnectionUpgrade,
};
use protocol::{Ping, PingDialer, PingOutput};
use std::{
    io, mem,
    time::{Duration, Instant},
};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Delay;
use void::Void;

/// Protocol handler that handles pinging the remote at a regular period.
///
/// If the remote doesn't respond, produces `Unresponsive` and closes the connection.
pub struct PeriodicPingHandler<TSubstream> {
    /// Configuration for the ping protocol.
    ping_config: toggleable::Toggleable<Ping<Instant>>,

    /// State of the outgoing ping.
    out_state: OutState<TSubstream>,

    /// Duration after which we consider that a ping failed.
    ping_timeout: Duration,

    /// After a ping succeeded, wait this long before the next ping.
    delay_to_next_ping: Duration,

    /// If true, we switch to the `Disabled` state if the remote doesn't support the ping protocol.
    /// If false, we close the connection.
    tolerate_unsupported: bool,
}

/// State of the outgoing ping substream.
enum OutState<TSubstream> {
    /// We need to open a new substream.
    NeedToOpen {
        /// Timeout after which we decide that it's not going to work out.
        ///
        /// Theoretically the handler should be polled immediately after we set the state to
        /// `NeedToOpen` and then we immediately transition away from it. However if the local node
        /// is for some reason busy, creating the `Delay` here avoids being overly generous with
        /// the ping timeout.
        expires: Delay,
    },

    /// Upgrading a substream to use ping.
    ///
    /// We produced a substream open request, and are waiting for it to be upgraded to a full
    /// ping-powered substream.
    Upgrading {
        /// Timeout after which we decide that it's not going to work out.
        ///
        /// The user of the `ProtocolsHandler` should ensure that there's a timeout when upgrading,
        /// but by storing a timeout here as well we ensure that we keep track of how long the
        /// ping has lasted.
        expires: Delay,
    },

    /// We sent a ping and we are waiting for the pong.
    WaitingForPong {
        /// Substream where we should receive the pong.
        substream: PingDialer<TSubstream, Instant>,
        /// Timeout after which we decide that we're not going to receive the pong.
        expires: Delay,
    },

    /// We received a pong and now we have nothing to do except wait a bit before sending the
    /// next ping.
    Idle {
        /// The substream to use to send pings.
        substream: PingDialer<TSubstream, Instant>,
        /// When to send the ping next.
        next_ping: Delay,
    },

    /// The ping dialer is disabled. Don't do anything.
    Disabled,

    /// The dialer has been closed.
    Shutdown,

    /// Something bad happened during the previous polling.
    Poisoned,
}

/// Event produced by the periodic pinger.
#[derive(Debug, Copy, Clone)]
pub enum OutEvent {
    /// The node has been determined to be unresponsive.
    Unresponsive,

    /// Started pinging the remote. This can be used to print a diagnostic message in the logs.
    PingStart,

    /// The node has successfully responded to a ping.
    PingSuccess(Duration),
}

impl<TSubstream> PeriodicPingHandler<TSubstream> {
    /// Builds a new `PeriodicPingHandler`.
    pub fn new() -> PeriodicPingHandler<TSubstream> {
        let ping_timeout = Duration::from_secs(30);

        PeriodicPingHandler {
            ping_config: toggleable::toggleable(Default::default()),
            out_state: OutState::NeedToOpen {
                expires: Delay::new(Instant::now() + ping_timeout),
            },
            ping_timeout,
            delay_to_next_ping: Duration::from_secs(15),
            tolerate_unsupported: false,
        }
    }
}

impl<TSubstream> Default for PeriodicPingHandler<TSubstream> {
    #[inline]
    fn default() -> Self {
        PeriodicPingHandler::new()
    }
}

impl<TSubstream> ProtocolsHandler for PeriodicPingHandler<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type InEvent = Void;
    type OutEvent = OutEvent;
    type Substream = TSubstream;
    type Protocol = toggleable::Toggleable<Ping<Instant>>;
    type OutboundOpenInfo = ();

    #[inline]
    fn listen_protocol(&self) -> Self::Protocol {
        let mut config = self.ping_config;
        config.disable();
        config
    }

    fn inject_fully_negotiated(
        &mut self,
        protocol: <Self::Protocol as ConnectionUpgrade<TSubstream>>::Output,
        _endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>,
    ) {
        match protocol {
            PingOutput::Pinger(mut substream) => {
                debug_assert!(_endpoint.is_dialer());
                match mem::replace(&mut self.out_state, OutState::Poisoned) {
                    OutState::Upgrading { expires } => {
                        // We always upgrade with the intent of immediately pinging.
                        substream.ping(Instant::now());
                        self.out_state = OutState::WaitingForPong { substream, expires };
                    }
                    _ => (),
                }
            }
            PingOutput::Ponger(_) => {
                debug_assert!(false, "Received an unexpected incoming ping substream");
            }
        }
    }

    fn inject_event(&mut self, _: &Self::InEvent) {}

    fn inject_inbound_closed(&mut self) {}

    #[inline]
    fn inject_dial_upgrade_error(&mut self, _: Self::OutboundOpenInfo, _: io::Error) {
        // In case of error while upgrading, there's not much we can do except shut down.
        // TODO: we assume that the error is about ping not being supported, which is not
        //       necessarily the case
        if self.tolerate_unsupported {
            self.out_state = OutState::Disabled;
        } else {
            self.out_state = OutState::Shutdown;
        }
    }

    fn shutdown(&mut self) {
        // Put `Shutdown` in `self.out_state` if we don't have any substream open.
        // Otherwise, keep the state as it is but call `shutdown()` on the substream. This
        // guarantees that the dialer will return `None` at some point.
        match self.out_state {
            OutState::WaitingForPong {
                ref mut substream, ..
            } => substream.shutdown(),
            OutState::Idle {
                ref mut substream, ..
            } => substream.shutdown(),
            ref mut s => *s = OutState::Shutdown,
        }
    }

    fn poll(
        &mut self,
    ) -> Poll<
        Option<ProtocolsHandlerEvent<Self::Protocol, Self::OutboundOpenInfo, Self::OutEvent>>,
        io::Error,
    > {
        // Shortcut for polling a `tokio_timer::Delay`
        macro_rules! poll_delay {
            ($delay:expr => { NotReady => $notready:expr, Ready => $ready:expr, }) => (
                match $delay.poll() {
                    Ok(Async::NotReady) => $notready,
                    Ok(Async::Ready(())) => $ready,
                    Err(err) => {
                        warn!(target: "sub-libp2p", "Ping timer errored: {:?}", err);
                        return Err(io::Error::new(io::ErrorKind::Other, err));
                    }
                }
            )
        }

        loop {
            match mem::replace(&mut self.out_state, OutState::Poisoned) {
                OutState::Shutdown | OutState::Poisoned => {
                    // This shuts down the whole connection with the remote.
                    return Ok(Async::Ready(None));
                },

                OutState::Disabled => {
                    return Ok(Async::NotReady);
                }

                // Need to open an outgoing substream.
                OutState::NeedToOpen { expires } => {
                    // Note that we ignore the expiration here, as it's pretty unlikely to happen.
                    // The expiration is only here to be transmitted to the `Upgrading`.
                    self.out_state = OutState::Upgrading { expires };
                    return Ok(Async::Ready(Some(
                        ProtocolsHandlerEvent::OutboundSubstreamRequest {
                            upgrade: self.ping_config,
                            info: (),
                        },
                    )));
                }

                // Waiting for the upgrade to be negotiated.
                OutState::Upgrading { mut expires } => poll_delay!(expires => {
                        NotReady => {
                            self.out_state = OutState::Upgrading { expires };
                            return Ok(Async::NotReady);
                        },
                        Ready => {
                            self.out_state = OutState::Shutdown;
                            let ev = OutEvent::Unresponsive;
                            return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(ev))));
                        },
                    }),

                // Waiting for the pong.
                OutState::WaitingForPong {
                    mut substream,
                    mut expires,
                } => {
                    // We start by dialing the substream, leaving one last chance for it to
                    // produce the pong even if the expiration happened.
                    match substream.poll()? {
                        Async::Ready(Some(started)) => {
                            self.out_state = OutState::Idle {
                                substream,
                                next_ping: Delay::new(Instant::now() + self.delay_to_next_ping),
                            };
                            let ev = OutEvent::PingSuccess(started.elapsed());
                            return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(ev))));
                        }
                        Async::NotReady => {}
                        Async::Ready(None) => {
                            self.out_state = OutState::Shutdown;
                            return Ok(Async::Ready(None));
                        }
                    };

                    // Check the expiration.
                    poll_delay!(expires => {
                        NotReady => {
                            self.out_state = OutState::WaitingForPong { substream, expires };
                            // Both `substream` and `expires` and not ready, so it's fine to return
                            // not ready.
                            return Ok(Async::NotReady);
                        },
                        Ready => {
                            self.out_state = OutState::Shutdown;
                            let ev = OutEvent::Unresponsive;
                            return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(ev))));
                        },
                    })
                }

                OutState::Idle {
                    mut substream,
                    mut next_ping,
                } => {
                    // Poll the future that fires when we need to ping the node again.
                    poll_delay!(next_ping => {
                        NotReady => {
                            self.out_state = OutState::Idle { substream, next_ping };
                            return Ok(Async::NotReady);
                        },
                        Ready => {
                            let expires = Delay::new(Instant::now() + self.ping_timeout);
                            substream.ping(Instant::now());
                            self.out_state = OutState::WaitingForPong { substream, expires };
                            return Ok(Async::Ready(Some(ProtocolsHandlerEvent::Custom(OutEvent::PingStart))));
                        },
                    })
                }
            }
        }
    }
}
