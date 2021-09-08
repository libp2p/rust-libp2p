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

pub(crate) struct ConcurrentDial<TMuxer, TError> {
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
        println!("Length of addresses: {}", addresses.len());
        let dials = FuturesUnordered::default();
        let mut errors = vec![];

        for address in addresses.into_iter().map(|a| p2p_addr(peer, a).unwrap()) {
            match transport.clone().dial(address.clone()) {
                Ok(fut) => dials.push(fut.map(|r| (address, r)).boxed()),
                Err(err) => errors.push((address, err)),
            }
        }

        Self { dials, errors }
    }
}

impl<TMuxer, TError> Future for ConcurrentDial<TMuxer, TError> {
    type Output = Result<(PeerId, Multiaddr, TMuxer), Vec<(Multiaddr, TransportError<TError>)>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        println!("Lenght of dials: {}", self.dials.len());
        loop {
            match ready!(self.dials.poll_next_unpin(cx)) {
                // TODO: What about self.errors? Sure we should loose these?
                Some((addr, Ok((peer_id, muxer)))) => {
                    println!("Got a connection");
                    return Poll::Ready(Ok((peer_id, addr, muxer)));
                }
                Some((addr, Err(e))) => {
                    println!("Got an error");
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
