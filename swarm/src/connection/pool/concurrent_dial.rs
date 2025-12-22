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
    collections::HashMap,
    num::NonZeroU8,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    future::{BoxFuture, Future},
    ready,
    stream::{FuturesUnordered, StreamExt},
    FutureExt,
};
use futures_timer::Delay;
use libp2p_core::muxing::StreamMuxerBox;
use libp2p_identity::PeerId;

use super::DialRanker;
use crate::{transport::TransportError, Multiaddr};

pub(crate) type Dial = BoxFuture<
    'static,
    (
        Multiaddr,
        Result<(PeerId, StreamMuxerBox), TransportError<std::io::Error>>,
    ),
>;

pub(crate) struct ConcurrentDial {
    concurrency_factor: NonZeroU8,
    dials: FuturesUnordered<Dial>,
    pending_dials: Box<dyn Iterator<Item = (Multiaddr, Option<Duration>, Dial)> + Send>,
    errors: Vec<(Multiaddr, TransportError<std::io::Error>)>,
}

impl Unpin for ConcurrentDial {}

impl ConcurrentDial {
    pub(crate) fn new(
        pending_dials: Vec<(Multiaddr, Dial)>,
        concurrency_factor: NonZeroU8,
        dial_ranker: Option<Arc<DialRanker>>,
    ) -> Self {
        let dials = FuturesUnordered::new();
        let pending_dials: Vec<_> = if let Some(dial_ranker) = dial_ranker {
            let addresses = pending_dials.iter().map(|(k, _)| k.clone()).collect();
            let mut dials: HashMap<Multiaddr, Dial> = HashMap::from_iter(pending_dials);
            dial_ranker(addresses)
                .into_iter()
                .filter_map(|(addr, delay)| dials.remove(&addr).map(|dial| (addr, delay, dial)))
                .collect()
        } else {
            pending_dials
                .into_iter()
                .map(|(addr, dial)| (addr, None, dial))
                .collect()
        };
        Self {
            concurrency_factor,
            dials,
            errors: Default::default(),
            pending_dials: Box::new(pending_dials.into_iter()),
        }
    }

    fn dial_pending(&mut self) -> bool {
        if let Some((_, delay, dial)) = self.pending_dials.next() {
            self.dials.push(
                async move {
                    if let Some(delay) = delay {
                        Delay::new(delay).await;
                    }
                    dial.await
                }
                .boxed(),
            );
            true
        } else {
            false
        }
    }
}

impl Future for ConcurrentDial {
    type Output = Result<
        // Either one dial succeeded, returning the negotiated [`PeerId`], the address, the
        // muxer and the addresses and errors of the dials that failed before.
        (
            Multiaddr,
            (PeerId, StreamMuxerBox),
            Vec<(Multiaddr, TransportError<std::io::Error>)>,
        ),
        // Or all dials failed, thus returning the address and error for each dial.
        Vec<(Multiaddr, TransportError<std::io::Error>)>,
    >;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        loop {
            match ready!(self.dials.poll_next_unpin(cx)) {
                Some((addr, Ok(output))) => {
                    let errors = std::mem::take(&mut self.errors);
                    return Poll::Ready(Ok((addr, output, errors)));
                }
                Some((addr, Err(e))) => {
                    self.errors.push((addr, e));
                    self.dial_pending();
                }
                None => {
                    while self.dials.len() < self.concurrency_factor.get() as usize
                        && self.dial_pending()
                    {}

                    if self.dials.is_empty() {
                        return Poll::Ready(Err(std::mem::take(&mut self.errors)));
                    }
                }
            }
        }
    }
}
