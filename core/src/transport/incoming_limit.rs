// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! Applies a limit to the number of simultaneous incoming connections of
//! a transport.
//!
//! The limit is enforced by not polling the underlying listener.
//!
//! By design, we don't hold any pending undelivered incoming connection. In other words, if the
//! underlying listener produces a connection then we always immediately deliver it.
//!
//! Since we don't know in advance whether polling the underlying listener will produce an
//! incoming connection, properly enforcing the limit would mean that we have to serialize
//! polling the listeners. Only one listener could be polled at a time.
//!
//! We don't want that to happen. Therefore, we allow the limit to temporarily overflow. If the
//! limit is `N` and you have `L` active listeners, then the actual number of upgrades can go up
//! to `N + L`.

use crate::{Multiaddr, transport::{ListenerEvent, Transport, TransportError}};

use futures::prelude::*;
use parking_lot::Mutex;
use slab::Slab;
use std::{mem, pin::Pin, task::{Context, Poll, Waker}};
use std::sync::{atomic::{AtomicUsize, Ordering}, Arc};

/// Structure shared between all the instances of [`IncomingLimitApply`] that
/// must share a limit.
pub struct IncomingLimit {
    /// Limit decided by the user. Never modified.
    limit: usize,

    /// Current number of connections being upgraded, plus number of listeners
    /// being polled.
    ///
    /// This is an optimization. This value must always be superior or equal
    /// to [`IncomingLimitInner::current_num`]. If `current_num_approx` is
    /// inferior to `limit`, then we know that `current_num` is also inferior
    /// to `limit`.
    current_num_approx: AtomicUsize,

    /// Struct behind a mutex.
    inner: Mutex<IncomingLimitInner>,
}

struct IncomingLimitInner {
    /// Current number of connections being upgraded.
    current_num: usize,

    /// Collection of wakers filled if the limit is reached. Each listener
    /// currently waiting for the limit may have an entry here, whose index
    /// is [`IncomingLimitListen::waker_slot_num`].
    wakers: Slab<Option<Waker>>,
}

impl IncomingLimit {
    /// Creates a new [`IncomingLimit`] that applies the given limit.
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(IncomingLimit {
            limit,
            current_num_approx: AtomicUsize::new(0),
            inner: Mutex::new(IncomingLimitInner {
                current_num: 0,
                wakers: Slab::new(),
            })
        })
    }
}

/// Applies the [`IncomingLimit`] to a transport.
pub(crate) fn incoming_limit<T>(transport: T, limit: Arc<IncomingLimit>) -> IncomingLimitApply<T> {
    IncomingLimitApply {
        inner: transport,
        limit,
    }
}

/// Wraps around an implementation of [`Transport`] and applies the limit to
/// the number of incoming connections.
pub struct IncomingLimitApply<TInner> {
    /// Underlying transport.
    inner: TInner,
    /// Limit system.
    limit: Arc<IncomingLimit>,
}

impl<TInner> Transport for IncomingLimitApply<TInner>
where
    TInner: Transport,
{
    type Output = TInner::Output;
    type Error = TInner::Error;
    type Listener = IncomingLimitListen<TInner::Listener>;
    type ListenerUpgrade = IncomingLimitUpgrade<TInner::ListenerUpgrade>;
    type Dial = TInner::Dial;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        Ok(IncomingLimitListen {
            inner: self.inner.listen_on(addr)?,
            limit: self.limit.clone(),
            waker_slot_num: None,
        })
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.inner.dial(addr)
    }
}

impl<TInner> Clone for IncomingLimitApply<TInner>
where
    TInner: Clone
{
    fn clone(&self) -> Self {
        IncomingLimitApply {
            inner: self.inner.clone(),
            limit: self.limit.clone(),
        }
    }
}

/// Reservation for a slot towards the limit.
struct NumApproxGuard<'a> {
    limit: &'a IncomingLimit,
}

impl<'a> NumApproxGuard<'a> {
    fn acquire(limit: &'a IncomingLimit) -> (Self, usize) {
        let current_num = limit.current_num_approx.fetch_add(1, Ordering::SeqCst);
        debug_assert_ne!(current_num, usize::max_value()); // Check for overflows.
        let guard = NumApproxGuard { limit };
        (guard, current_num)
    }
}

impl<'a> Drop for NumApproxGuard<'a> {
    fn drop(&mut self) {
        self.limit.current_num_approx.fetch_sub(1, Ordering::SeqCst);
    }
}

#[must_use]
#[pin_project::pin_project(PinnedDrop)]
pub struct IncomingLimitListen<TInner> {
    #[pin]
    inner: TInner,
    limit: Arc<IncomingLimit>,
    /// Entry number reserved to us within `limit.inner.wakers`. Starts with `None`, then
    /// assigned only if necessary. Once it is `Some`, the value is never changed.
    waker_slot_num: Option<usize>,
}

impl<TInner, TUpgr> Stream for IncomingLimitListen<TInner>
where
    TInner: TryStream<Ok = ListenerEvent<TUpgr>>,
{
    type Item = Result<ListenerEvent<IncomingLimitUpgrade<TUpgr>>, TInner::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = self.project();

        // We increase `current_num_approx` by one.
        // The `_guard` will make sure that we decrease it again before leaving this function.
        let (_guard, old_current_num_approx) = NumApproxGuard::acquire(&this.limit);
        if old_current_num_approx >= this.limit.limit {
            // `current_num_approx` is over the limit. We lock the mutex in order to check
            // whether we are actually indeed over the limit.
            let mut inner = this.limit.inner.lock();
            if inner.current_num >= this.limit.limit {
                // We are over the limit. Register a waker and return `Pending`.
                if let Some(waker_slot_num) = this.waker_slot_num {
                    let our_slot = &mut inner.wakers[*waker_slot_num];
                    if our_slot.as_ref().map(|s| !cx.waker().will_wake(s)).unwrap_or(true) {
                        *our_slot = Some(cx.waker().clone());
                    }
                } else {
                    let waker_slot_num = inner.wakers.insert(Some(cx.waker().clone()));
                    *this.waker_slot_num = Some(waker_slot_num);
                }

                return Poll::Pending;
            }
        };

        // We know that we are under the limit. We can poll the listener.
        // Note that `_guard` is still alive here.
        let (upgrade, local_addr, remote_addr) = match this.inner.try_poll_next(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(None) => return Poll::Ready(None),
            Poll::Ready(Some(Ok(ListenerEvent::NewAddress(addr)))) =>
                return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(addr)))),
            Poll::Ready(Some(Ok(ListenerEvent::AddressExpired(addr)))) =>
                return Poll::Ready(Some(Ok(ListenerEvent::AddressExpired(addr)))),
            Poll::Ready(Some(Err(err))) =>
                return Poll::Ready(Some(Err(err))),
            Poll::Ready(Some(Ok(ListenerEvent::Upgrade { upgrade, local_addr, remote_addr }))) => {
                (upgrade, local_addr, remote_addr)
            }
        };

        // Properly reserve our slot. Requires a mutex lock.
        mem::forget(_guard);
        let mut inner_lock = this.limit.inner.lock();
        //debug_assert!(inner_lock.current_num < this.limit.limit);
        //debug_assert!(inner_lock.current_num < this.limit.current_num_approx.load(Ordering::Relaxed));
        inner_lock.current_num += 1;

        Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
            upgrade: IncomingLimitUpgrade {
                inner: upgrade,
                cleaned_up: false,
                limit: this.limit.clone(),
            },
            local_addr,
            remote_addr,
        })))
    }
}

#[pin_project::pinned_drop]
impl<TInner> PinnedDrop for IncomingLimitListen<TInner> {
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();
        if let Some(waker_slot_num) = this.waker_slot_num {
            this.limit.inner.lock().wakers.remove(*waker_slot_num);
        }
    }
}

/// Wraps around the [`Transport::ListenerUpgrade`] of the underlying transport where an
/// incoming limit is applied.
#[must_use]
#[pin_project::pin_project(PinnedDrop)]
pub struct IncomingLimitUpgrade<TInner> {
    #[pin]
    inner: TInner,

    /// True if we already free'd a slot in `limit`.
    cleaned_up: bool,

    /// This struct implicitely holds a slot in both `current_num` and `current_num_approx`.
    /// Dropping this struct must decrease these two values by one.
    limit: Arc<IncomingLimit>,
}

impl<TInner> Future for IncomingLimitUpgrade<TInner>
where
    TInner: Future,
{ 
    type Output = TInner::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match self.as_mut().project().inner.poll(cx) {
            Poll::Ready(v) => {
                IncomingLimitUpgrade::cleanup(self);
                Poll::Ready(v)
            },
            Poll::Pending => Poll::Pending,
        }
    }
}

#[pin_project::pinned_drop]
impl<TInner> PinnedDrop for IncomingLimitUpgrade<TInner> {
    fn drop(self: Pin<&mut Self>) {
        self.cleanup();
    }
}

impl<TInner> IncomingLimitUpgrade<TInner> {
    fn cleanup(self: Pin<&mut Self>) {
        let this = self.project();
        if *this.cleaned_up {
            return;
        }
        *this.cleaned_up = true;

        let mut inner = this.limit.inner.lock();
        debug_assert_ne!(inner.current_num, 0);

        // By contract, `current_num` must be decreased before `current_num_approx`.
        inner.current_num -= 1;

        // If decreasing `current_num` makes us drop below the limit, we wake up all the wakers so
        // that other listeners can start their work.
        if inner.current_num == this.limit.limit.saturating_sub(1) {
            for (_, waker) in &mut inner.wakers {
                if let Some(waker) = waker.take() {
                    waker.wake();
                }
            }
        }

        let old_val = this.limit.current_num_approx.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(old_val > inner.current_num);
        debug_assert_ne!(old_val, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::{incoming_limit, IncomingLimit, IncomingLimitApply};
    use crate::{Multiaddr, transport::{ListenerEvent, Transport, TransportError}};
    use futures::prelude::*;
    use futures_timer::Delay;
    use std::{pin::Pin, sync::{atomic, Arc}, task::Poll, time::Duration};

    #[test]
    fn incoming_limit_working() {
        const LIMIT: usize = 5;
        const PARALLELISM: usize = 20;
        const MAX_POSSIBLE_VALUE: usize = LIMIT + PARALLELISM;

        struct DummyTansport(Arc<atomic::AtomicUsize>);
        impl Transport for DummyTansport {
            type Output = ();
            type Error = std::io::Error;
            type Listener = Pin<Box<dyn Stream<Item = Result<ListenerEvent<Self::ListenerUpgrade>, Self::Error>> + Send>>;
            type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
            type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
        
            fn listen_on(self, _: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
                let num = self.0;
                Ok(Box::pin(stream::unfold((), move |()| {
                    let num = num.clone();
                    async move {
                        let num = num.clone();
                        let ret = Ok(ListenerEvent::Upgrade {
                            upgrade: Box::pin(async move {
                                assert!(num.fetch_add(1, atomic::Ordering::Acquire) + 1 <= MAX_POSSIBLE_VALUE);
                                // Yield a couple times.
                                let mut n = 0;
                                future::poll_fn(|cx| {
                                    n += 1;
                                    if n >= 50 {
                                        Poll::Ready(())
                                    } else {
                                        cx.waker().wake_by_ref();
                                        Poll::Pending
                                    }
                                }).await;
                                assert!(num.fetch_sub(1, atomic::Ordering::Release) <= MAX_POSSIBLE_VALUE);
                                Ok(())
                            }) as Pin<Box<dyn Future<Output = _> + Send>>,
                            local_addr: "/memory/1234".parse().unwrap(),
                            remote_addr: "/memory/1234".parse().unwrap(),
                        });

                        Some((ret, ()))
                    }
                })) as Pin<Box<_>>)
            }
        
            fn dial(self, _: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
                panic!()
            }
        }

        let limit = IncomingLimit::new(LIMIT);
        let num = Arc::new(atomic::AtomicUsize::new(0));

        // We spawn many tasks and listen in parallel as fast as possible.
        // The code of `DummyTransport` contains assertions that verify that we stay below the
        // limit.

        let mut handles = Vec::new();
        for _ in 0..PARALLELISM {
            let transport = incoming_limit(DummyTansport(num.clone()), limit.clone());
            handles.push(async_std::task::spawn(async move {
                let mut listener = transport.listen_on("/memory/1234".parse().unwrap()).unwrap();
                for _ in 0..100 {
                    let (upgrade, _) = listener.next().await.unwrap().unwrap()
                        .into_upgrade().unwrap();
                    upgrade.await.unwrap();
                }
            }));
        }

        for handle in handles {
            async_std::task::block_on(handle);
        }
    }
}
