// Copyright 2021 Protocol Labs.
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

pub use crate::connection::{ConnectionCounters, ConnectionLimits};

use crate::{
    transport::{Transport, TransportError},
    Multiaddr, PeerId,
};
use futures::{
    future::{BoxFuture, Future, FutureExt},
    ready,
    stream::{FuturesUnordered, StreamExt},
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

type Dial<TMuxer, TError> =
    BoxFuture<'static, (Multiaddr, Result<(PeerId, TMuxer), TransportError<TError>>)>;

pub struct ConcurrentDial<TMuxer, TError> {
    dials: FuturesUnordered<Dial<TMuxer, TError>>,
    pending_dials: Box<dyn Iterator<Item = Dial<TMuxer, TError>> + Send>,
    errors: Vec<(Multiaddr, TransportError<TError>)>,
}

impl<TMuxer, TError> Unpin for ConcurrentDial<TMuxer, TError> {}

impl<TMuxer: Send + 'static, TError: Send + 'static> ConcurrentDial<TMuxer, TError> {
    pub(crate) fn new<TTrans: Send>(
        transport: TTrans,
        peer: Option<PeerId>,
        addresses: Vec<Multiaddr>,
    ) -> Self
    where
        TTrans: Transport<Output = (PeerId, TMuxer), Error = TError> + Clone + 'static,
        TTrans::Dial: Send + 'static,
    {
        let mut pending_dials = addresses.into_iter().map(move |address| {
            match p2p_addr(peer, address) {
                Ok(address) => match transport.clone().dial(address.clone()) {
                    Ok(fut) => fut
                        .map(|r| (address, r.map_err(|e| TransportError::Other(e))))
                        .boxed(),
                    Err(err) => futures::future::ready((address.clone(), Err(err))).boxed(),
                },
                Err(address) => {
                    futures::future::ready((
                        address.clone(),
                        // TODO: It is not really the Multiaddr that is not supported.
                        Err(TransportError::MultiaddrNotSupported(address)),
                    ))
                    .boxed()
                }
            }
        });

        let dials = FuturesUnordered::new();
        while let Some(dial) = pending_dials.next() {
            dials.push(dial);
            if dials.len() == 5 {
                break;
            }
        }

        Self {
            dials,
            errors: Default::default(),
            pending_dials: Box::new(pending_dials),
        }
    }
}

impl<TMuxer, TError> Future for ConcurrentDial<TMuxer, TError> {
    type Output = Result<
        // Either one dial succeeded, returning the negotiated [`PeerId`], the address, the
        // muxer and the addresses and errors of the dials that failed before.
        (
            PeerId,
            Multiaddr,
            TMuxer,
            Vec<(Multiaddr, TransportError<TError>)>,
        ),
        // Or all dials failed, thus returning the address and error for each dial.
        Vec<(Multiaddr, TransportError<TError>)>,
    >;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            match ready!(self.dials.poll_next_unpin(cx)) {
                Some((addr, Ok((peer_id, muxer)))) => {
                    let errors = std::mem::replace(&mut self.errors, vec![]);
                    return Poll::Ready(Ok((peer_id, addr, muxer, errors)));
                }
                Some((addr, Err(e))) => {
                    self.errors.push((addr, e));
                    if let Some(dial) = self.pending_dials.next() {
                        self.dials.push(dial)
                    }
                }
                None => {
                    return Poll::Ready(Err(std::mem::replace(&mut self.errors, vec![])));
                }
            }
        }
    }
}

/// Ensures a given `Multiaddr` is a `/p2p/...` address for the given peer.
///
/// If the given address is already a `p2p` address for the given peer,
/// i.e. the last encapsulated protocol is `/p2p/<peer-id>`, this is a no-op.
///
/// If the given address is already a `p2p` address for a different peer
/// than the one given, the given `Multiaddr` is returned as an `Err`.
///
/// If the given address is not yet a `p2p` address for the given peer,
/// the `/p2p/<peer-id>` protocol is appended to the returned address.
fn p2p_addr(peer: Option<PeerId>, addr: Multiaddr) -> Result<Multiaddr, Multiaddr> {
    // TODO: Make hack nicer.
    let peer = match peer {
        Some(p) => p,
        None => return Ok(addr),
    };

    if let Some(multiaddr::Protocol::P2p(hash)) = addr.iter().last() {
        if &hash != peer.as_ref() {
            return Err(addr);
        }
        Ok(addr)
    } else {
        Ok(addr.with(multiaddr::Protocol::P2p(peer.into())))
    }
}
