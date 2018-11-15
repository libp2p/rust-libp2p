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

//! Contains the `IdentifyTransport` type.

use futures::prelude::*;
use libp2p_core::{
    Multiaddr, PeerId, PublicKey, muxing, Transport,
    upgrade::{self, OutboundUpgradeApply, UpgradeError}
};
use protocol::{RemoteInfo, IdentifyProtocolConfig};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::mem;
use std::sync::Arc;

/// Wraps around an implementation of `Transport` that yields a muxer. Will use the muxer to
/// open a substream with the remote and retreive its peer id. Then yields a
/// `(PeerId, impl StreamMuxer)`.
///
/// This transport can be used if you don't use any encryption layer, or if you want to make
/// encryption optional, in which case you have no other way to know the `PeerId` of the remote
/// than to ask for it.
///
/// > **Note**: If you use this transport, keep in mind that the `PeerId` returned by the remote
/// >           can be anything and shouldn't necessarily be trusted.
#[derive(Debug, Clone)]
pub struct IdentifyTransport<TTrans> {
    /// The underlying transport we wrap around.
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
        Err((self, addr))
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
            IdRetriever::new(muxer, IdentifyProtocolConfig).map_err(|e| e.into_io_error())
        });

        Ok(Box::new(dial) as Box<_>)
    }

    #[inline]
    fn nat_traversal(&self, a: &Multiaddr, b: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(a, b)
    }
}

/// Implementation of `Future` that asks the remote of its `PeerId`.
// TODO: remove unneeded bounds
struct IdRetriever<TMuxer>
where TMuxer: muxing::StreamMuxer + Send + Sync + 'static,
      TMuxer::Substream: Send,
{
    /// Internal state.
    state: IdRetrieverState<TMuxer>
}

enum IdRetrieverState<TMuxer>
where TMuxer: muxing::StreamMuxer + Send + Sync + 'static,
      TMuxer::Substream: Send,
{
    /// We are in the process of opening a substream with the remote.
    OpeningSubstream(Arc<TMuxer>, muxing::OutboundSubstreamRefWrapFuture<Arc<TMuxer>>, IdentifyProtocolConfig),
    /// We opened the substream and are currently negotiating the identify protocol.
    NegotiatingIdentify(Arc<TMuxer>, OutboundUpgradeApply<muxing::SubstreamRef<Arc<TMuxer>>, IdentifyProtocolConfig>),
    /// We retreived the remote's public key and are ready to yield it when polled again.
    Finishing(Arc<TMuxer>, PublicKey),
    /// Something bad happend, or the `Future` is finished, and shouldn't be polled again.
    Poisoned,
}

impl<TMuxer> IdRetriever<TMuxer>
where TMuxer: muxing::StreamMuxer + Send + Sync + 'static,
      TMuxer::Substream: Send,
{
    /// Creates a new `IdRetriever` ready to be polled.
    fn new(muxer: TMuxer, config: IdentifyProtocolConfig) -> Self {
        let muxer = Arc::new(muxer);
        let opening = muxing::outbound_from_ref_and_wrap(muxer.clone());

        IdRetriever {
            state: IdRetrieverState::OpeningSubstream(muxer, opening, config)
        }
    }
}

impl<TMuxer> Future for IdRetriever<TMuxer>
where TMuxer: muxing::StreamMuxer + Send + Sync + 'static,
      TMuxer::Substream: Send,
{
    type Item = (PeerId, TMuxer);
    type Error = UpgradeError<IoError>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        // This loop is here so that we can continue polling until we're ready.
        loop {
            // In order to satisfy the borrow checker, we extract the state and temporarily put
            // `Poisoned` instead.
            match mem::replace(&mut self.state, IdRetrieverState::Poisoned) {
                IdRetrieverState::OpeningSubstream(muxer, mut opening, config) => {
                    match opening.poll() {
                        Ok(Async::Ready(Some(substream))) => {
                            let upgrade = upgrade::apply_outbound(substream, config);
                            self.state = IdRetrieverState::NegotiatingIdentify(muxer, upgrade)
                        },
                        Ok(Async::Ready(None)) => {
                            return Err(UpgradeError::Apply(IoError::new(IoErrorKind::Other, "remote refused our identify attempt")))
                        }
                        Ok(Async::NotReady) => {
                            self.state = IdRetrieverState::OpeningSubstream(muxer, opening, config);
                            return Ok(Async::NotReady);
                        },
                        Err(err) => return Err(UpgradeError::Apply(err))
                    }
                },
                IdRetrieverState::NegotiatingIdentify(muxer, mut nego) => {
                    match nego.poll() {
                        Ok(Async::Ready(RemoteInfo { info, .. })) => {
                            self.state = IdRetrieverState::Finishing(muxer, info.public_key);
                        },
                        Ok(Async::NotReady) => {
                            self.state = IdRetrieverState::NegotiatingIdentify(muxer, nego);
                            return Ok(Async::NotReady);
                        },
                        Err(err) => return Err(err),
                    }
                },
                IdRetrieverState::Finishing(muxer, public_key) => {
                    // Here is a tricky part: we need to get back the muxer in order to return
                    // it, but it is in an `Arc`.
                    let unwrapped = Arc::try_unwrap(muxer).unwrap_or_else(|_| {
                        panic!("we clone the Arc only to put it into substreams ; once in the \
                                Finishing state, no substream or upgrade exists anymore ; \
                                therefore there exists only one instance of the Arc ; qed")
                    });

                    // We leave `Poisoned` as the state when returning.
                    return Ok(Async::Ready((public_key.into(), unwrapped)));
                },
                IdRetrieverState::Poisoned => {
                    panic!("Future state panicked inside poll() or is finished")
                },
            }
        }
    }
}

// TODO: write basic working test
