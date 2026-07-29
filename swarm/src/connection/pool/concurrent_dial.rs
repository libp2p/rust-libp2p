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

use std::{
    num::NonZeroU8,
    pin::Pin,
    task::{Context, Poll},
    vec::IntoIter,
};

use futures::{
    FutureExt,
    future::{BoxFuture, Future},
    ready,
    stream::{FuturesUnordered, StreamExt},
};
use futures_timer::Delay;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_identity::PeerId;

use crate::{Multiaddr, connection::pool::dial_ranker::rank_dials, transport::TransportError};

/// A pending outbound dial that hasn't started yet.
///
/// The `fut` is created upfront by `Transport::dial()` but won't be polled
/// until `delay` elapses. If `smart_dial` is enabled, `rank_dials` sets the
/// `delay` and reorders the vec so preferred addresses start first. Without
/// smart dial, delays are `None` and the concurrency factor limits how many
/// are polled at once.
pub(crate) struct PendingDial {
    pub(crate) addr: Multiaddr,
    pub(crate) fut: DialFuture,
}

/// The async dial operation. Owns the actual transport dial and returns the
/// dialed address along with the result so the pool can match failures
/// back to specific addresses.
pub(crate) type DialFuture = BoxFuture<
    'static,
    (
        Multiaddr,
        Result<(PeerId, StreamMuxerBox), TransportError<std::io::Error>>,
    ),
>;

/// The result of a concurrent or smart dial to a single peer.
///
/// Returns the first successful address and its negotiated connection, along
/// with any errors from earlier failed dials. Returns all errors if every
/// dial failed.
pub(crate) type DialResult = Result<
    (
        Multiaddr,
        (PeerId, StreamMuxerBox),
        Vec<(Multiaddr, TransportError<std::io::Error>)>,
    ),
    Vec<(Multiaddr, TransportError<std::io::Error>)>,
>;

/// Drives concurrent dial attempts to a single peer, limited by a concurrency factor.
///
/// Starts up to `concurrency_factor` dials simultaneously. On failure, the next
/// pending dial starts immediately, keeping the concurrency window filled until
/// either a dial succeeds or all pending dials are exhausted.
pub(crate) struct ConcurrentDial {
    dials: FuturesUnordered<DialFuture>,
    pending_dials: IntoIter<PendingDial>,
    errors: Vec<(Multiaddr, TransportError<std::io::Error>)>,
}

impl Unpin for ConcurrentDial {}

impl ConcurrentDial {
    pub(crate) fn new(pending_dials: Vec<PendingDial>, concurrency_factor: NonZeroU8) -> Self {
        let mut pending_dials = pending_dials.into_iter();

        // Fill the concurrency window: start up to `concurrency_factor` dials.
        // As each dial completes (success or failure), `dial_pending()` is called
        // to refill the window from remaining pending dials.
        let dials = FuturesUnordered::new();
        for dial in pending_dials
            .by_ref()
            .take(concurrency_factor.get() as usize)
        {
            dials.push(dial.fut);
        }

        Self {
            dials,
            errors: Default::default(),
            pending_dials,
        }
    }
}

impl Future for ConcurrentDial {
    type Output = DialResult;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            match ready!(self.dials.poll_next_unpin(cx)) {
                Some((addr, Ok(output))) => {
                    let errors = std::mem::take(&mut self.errors);
                    return Poll::Ready(Ok((addr, output, errors)));
                }
                Some((addr, Err(e))) => {
                    self.errors.push((addr, e));
                    if let Some(dial) = self.pending_dials.next() {
                        self.dials.push(dial.fut);
                    }
                }
                None => {
                    return Poll::Ready(Err(std::mem::take(&mut self.errors)));
                }
            }
        }
    }
}

/// Drives ranked dial attempts to a single peer with staggered delays.
///
/// All dials start immediately via [`rank_dials`], which assigns delays based on
/// transport priority (QUIC > TCP, IPv6 > IPv4) and Happy Eyeballs (RFC 8305).
/// The delays pace the dials, giving faster transports a head start while slower
/// paths wait, with all dials overlapping in flight.
pub(crate) struct SmartDial {
    dials: FuturesUnordered<DialFuture>,
    errors: Vec<(Multiaddr, TransportError<std::io::Error>)>,
}

impl Unpin for SmartDial {}

impl SmartDial {
    pub(crate) fn new(pending_dials: Vec<PendingDial>) -> Self {
        let pending_dials = rank_dials(pending_dials);

        let dials = FuturesUnordered::new();
        for (delay, dial) in pending_dials {
            dials.push(
                async move {
                    if !delay.is_zero() {
                        Delay::new(delay).await;
                    }
                    dial.fut.await
                }
                .boxed(),
            );
        }

        Self {
            dials,
            errors: Default::default(),
        }
    }
}

impl Future for SmartDial {
    type Output = DialResult;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            match ready!(self.dials.poll_next_unpin(cx)) {
                Some((addr, Ok(output))) => {
                    let errors = std::mem::take(&mut self.errors);
                    return Poll::Ready(Ok((addr, output, errors)));
                }
                Some((addr, Err(e))) => {
                    self.errors.push((addr, e));
                }
                None => {
                    return Poll::Ready(Err(std::mem::take(&mut self.errors)));
                }
            }
        }
    }
}
