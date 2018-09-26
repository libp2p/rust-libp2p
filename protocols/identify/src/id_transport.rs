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
use libp2p_core::{Endpoint, Multiaddr, PeerId, PublicKey, Transport, muxing, upgrade::apply};
use protocol::{IdentifyOutput, IdentifyProtocolConfig};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::mem;
use std::sync::Arc;

/// Implementation of `Transport`. See [the crate root description](index.html).
#[derive(Debug, Clone)]
pub struct IdentifyTransport<TTrans> {
    transport: TTrans,
}

impl<TTrans> IdentifyTransport<TTrans> {
    /// Creates an `IdentifyTransport` that wraps around the given transport.
    #[inline]
    pub fn new(transport: TTrans) -> Self {
        IdentifyTransport {
            transport,
        }
    }
}

// TODO: don't use boxes
impl<TTrans, TMuxer> Transport for IdentifyTransport<TTrans>
where
    TTrans: Transport<Output = TMuxer>,
    TMuxer: muxing::StreamMuxer + Send + Sync + 'static,      // TODO: remove unnecessary bounds
    TMuxer::Substream: Send + Sync + 'static,      // TODO: remove unnecessary bounds
    TMuxer::OutboundSubstream: Send + 'static,      // TODO: remove unnecessary bounds
    TTrans::Dial: Send + Sync + 'static,
    TTrans::Listener: Send + 'static,
    TTrans::ListenerUpgrade: Send + 'static,
{
    type Output = (PeerId, TMuxer);
    type Listener = Box<Stream<Item = (Self::ListenerUpgrade, Multiaddr), Error = IoError> + Send>;
    type ListenerUpgrade = Box<Future<Item = Self::Output, Error = IoError> + Send>;
    type Dial = Box<Future<Item = Self::Output, Error = IoError> + Send>;

    #[inline]
    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let (listener, new_addr) = match self.transport.listen_on(addr) {
            Ok((l, a)) => (l, a),
            Err((inner, addr)) => {
                let id = IdentifyTransport {
                    transport: inner,
                };
                return Err((id, addr));
            }
        };

        let listener = listener
            .map(move |(upgrade, remote_addr)| {
                let addr = remote_addr.clone();
                let upgr = upgrade
                    .and_then(move |muxer| {
                        IdRetreiver::new(muxer, IdentifyProtocolConfig, Endpoint::Listener, addr)
                    });
                (Box::new(upgr) as Box<Future<Item = _, Error = _> + Send>, remote_addr)
            });

        Ok((Box::new(listener) as Box<_>, new_addr))
    }

    #[inline]
    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        // We dial a first time the node.
        let dial = match self.transport.dial(addr.clone()) {
            Ok(d) => d,
            Err((transport, addr)) => {
                let id = IdentifyTransport {
                    transport,
                };
                return Err((id, addr));
            }
        };

        let dial = dial.and_then(move |muxer| {
            IdRetreiver::new(muxer, IdentifyProtocolConfig, Endpoint::Dialer, addr)
        });

        Ok(Box::new(dial) as Box<_>)
    }

    #[inline]
    fn nat_traversal(&self, a: &Multiaddr, b: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(a, b)
    }
}

// TODO: remove unneeded bounds
struct IdRetreiver<TMuxer>
where TMuxer: muxing::StreamMuxer + Send + Sync + 'static,
      TMuxer::Substream: Send,
{
    muxer: Option<Arc<TMuxer>>,
    state: IdRetreiverState<TMuxer>,
    endpoint: Endpoint,
    remote_addr: Multiaddr,
}

enum IdRetreiverState<TMuxer>
where TMuxer: muxing::StreamMuxer + Send + Sync + 'static,
      TMuxer::Substream: Send,
{
    OpeningSubstream(muxing::OutboundSubstreamRefWrapFuture<Arc<TMuxer>>, IdentifyProtocolConfig),
    NegotiatingIdentify(apply::UpgradeApplyFuture<muxing::SubstreamRef<Arc<TMuxer>>, IdentifyProtocolConfig>),
    Finishing(PublicKey),
    Poisoned,
}

impl<TMuxer> IdRetreiver<TMuxer>
where TMuxer: muxing::StreamMuxer + Send + Sync + 'static,
      TMuxer::Substream: Send,
{
    fn new(muxer: TMuxer, config: IdentifyProtocolConfig, endpoint: Endpoint, remote_addr: Multiaddr) -> Self {
        let muxer = Arc::new(muxer);
        let opening = muxing::outbound_from_ref_and_wrap(muxer.clone());

        IdRetreiver {
            muxer: Some(muxer),
            state: IdRetreiverState::OpeningSubstream(opening, config),
            endpoint,
            remote_addr,
        }
    }
}

impl<TMuxer> Future for IdRetreiver<TMuxer>
where TMuxer: muxing::StreamMuxer + Send + Sync + 'static,
      TMuxer::Substream: Send,
{
    type Item = (PeerId, TMuxer);
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, IdRetreiverState::Poisoned) {
                IdRetreiverState::OpeningSubstream(mut opening, config) => {
                    match opening.poll() {
                        Ok(Async::Ready(Some(substream))) => {
                            let upgrade = apply::apply(substream, config, self.endpoint, &self.remote_addr);
                            self.state = IdRetreiverState::NegotiatingIdentify(upgrade);
                        },
                        Ok(Async::Ready(None)) => {
                            return Err(IoError::new(IoErrorKind::Other, "remote refused our identify attempt"));
                        },
                        Ok(Async::NotReady) => {
                            self.state = IdRetreiverState::OpeningSubstream(opening, config);
                            return Ok(Async::NotReady);
                        },
                        Err(err) => return Err(err),
                    }
                },
                IdRetreiverState::NegotiatingIdentify(mut nego) => {
                    match nego.poll() {
                        Ok(Async::Ready(IdentifyOutput::RemoteInfo { info, .. })) => {
                            self.state = IdRetreiverState::Finishing(info.public_key);
                        },
                        Ok(Async::Ready(IdentifyOutput::Sender { .. })) => {
                            unreachable!("IdentifyOutput::Sender can never be the output from \
                                          the dialing side");
                        },
                        Ok(Async::NotReady) => {
                            self.state = IdRetreiverState::NegotiatingIdentify(nego);
                            return Ok(Async::NotReady);
                        },
                        Err(err) => return Err(err),
                    }
                },
                IdRetreiverState::Finishing(public_key) => {
                    let muxer = self.muxer.take()
                        .expect("we only extract muxer when we are Finishing ; the Finishing \
                                 state transitions into Poisoned ; therefore muxer is only \
                                 extracted once ; qed");
                    let unwrapped = Arc::try_unwrap(muxer).unwrap_or_else(|_| {
                        panic!("we clone the Arc only to put it into substreams ; once in the \
                                Finishing state, no substream or upgrade exists anymore ; \
                                therefore there exists only one instance of the Arc ; qed")
                    });
                    return Ok(Async::Ready((public_key.into(), unwrapped)));
                },
                IdRetreiverState::Poisoned => {
                    panic!("Future state panicked inside poll() or is finished")
                },
            }
        }
    }
}

// TODO: write basic working test
