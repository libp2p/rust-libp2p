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
    muxing::StreamMuxer,
    transport::{Transport, TransportError},
    ConnectedPoint, Executor, Multiaddr, PeerId,
};
use fnv::FnvHashMap;
use futures::future::BoxFuture;
use futures::ready;
use futures::stream::FuturesUnordered;
use futures::{future, prelude::*};
use smallvec::SmallVec;
use std::{
    collections::hash_map,
    convert::TryFrom as _,
    error, fmt,
    num::{NonZeroU32, NonZeroUsize},
    pin::Pin,
    task::{Context, Poll},
};

// TODO: Have a concurrency limit.

// TODO: pub needed?
pub struct ConcurrentDial<TMuxer, TError> {
    // TODO: We could as well spawn each of these on a separate task.
    dials: FuturesUnordered<BoxFuture<'static, (Multiaddr, Result<(PeerId, TMuxer), TError>)>>,
    errors: Vec<(Multiaddr, TransportError<TError>)>,
}

impl<TMuxer, TError> Unpin for ConcurrentDial<TMuxer, TError> {}

impl<TMuxer, TError> ConcurrentDial<TMuxer, TError> {
    pub(crate) fn new<TTrans>(
        transport: TTrans,
        peer: Option<PeerId>,
        addresses: Vec<Multiaddr>,
    ) -> Self
    where
        TTrans: Transport<Output = (PeerId, TMuxer), Error = TError> + Clone,
        TTrans::Dial: Send + 'static,
        TError: std::fmt::Debug,
    {
        let dials = FuturesUnordered::default();
        let mut errors = vec![];

        for address in addresses.into_iter().map(|a| p2p_addr(peer, a)) {
            match address {
                Ok(address) => match transport.clone().dial(address.clone()) {
                    Ok(fut) => dials.push(fut.map(|r| (address, r)).boxed()),
                    Err(err) => errors.push((address, err)),
                },
                Err(address) => errors.push((
                    address.clone(),
                    // TODO: It is not really the Multiaddr that is not supported.
                    TransportError::MultiaddrNotSupported(address),
                )),
            }
        }

        Self { dials, errors }
    }
}

impl<TMuxer, TError> Future for ConcurrentDial<TMuxer, TError> {
    type Output = Result<
        (
            PeerId,
            Multiaddr,
            TMuxer,
            Vec<(Multiaddr, TransportError<TError>)>,
        ),
        Vec<(Multiaddr, TransportError<TError>)>,
    >;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            match ready!(self.dials.poll_next_unpin(cx)) {
                Some((addr, Ok((peer_id, muxer)))) => {
                    return Poll::Ready(Ok((
                        peer_id,
                        addr,
                        muxer,
                        std::mem::replace(&mut self.errors, vec![]),
                    )));
                }
                Some((addr, Err(e))) => {
                    self.errors.push((addr, TransportError::Other(e)));
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
