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

use fnv::FnvHashMap;
use futures::{future, sync::oneshot, task, Async, Future, Poll, IntoFuture};
use parking_lot::Mutex;
use {Multiaddr, MuxedTransport, SwarmController, Transport};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::mem;
use std::sync::{Arc, Weak, atomic::AtomicUsize, atomic::Ordering};
use transport::interruptible::Interrupt;

/// Storage for a unique connection with a remote.
pub struct UniqueConnec<T> {
    inner: Arc<Mutex<UniqueConnecInner<T>>>,
}

enum UniqueConnecInner<T> {
    /// The `UniqueConnec` was created, but nothing is in it.
    Empty,
    /// We started dialing, but no response has been obtained so far.
    Pending {
        /// Tasks that need to be awakened when the content of this object is set.
        tasks_waiting: FnvHashMap<usize, task::Task>,
        /// Future that represents when `tie_*` should have been called.
        // TODO: Send + Sync bound is meh
        dial_fut: Box<Future<Item = (), Error = IoError> + Send + Sync>,
        /// Dropping this object will automatically interrupt the dial, which is very useful if
        /// we clear or drop the `UniqueConnec`.
        interrupt: Interrupt,
    },
    /// The value of this unique connec has been set.
    /// Can only transition to `Empty` when the future has expired.
    Full {
        /// Content of the object.
        value: T,
        /// Sender to trigger if the content gets cleared.
        on_clear: oneshot::Sender<()>,
    },
    /// The `dial_fut` has errored.
    Errored(IoError),
}

impl<T> UniqueConnec<T> {
    /// Builds a new empty `UniqueConnec`.
    #[inline]
    pub fn empty() -> Self {
        UniqueConnec {
            inner: Arc::new(Mutex::new(UniqueConnecInner::Empty)),
        }
    }

    /// Builds a new `UniqueConnec` that contains a value.
    #[inline]
    pub fn with_value(value: T) -> Self {
        let (on_clear, _) = oneshot::channel();
        UniqueConnec {
            inner: Arc::new(Mutex::new(UniqueConnecInner::Full { value, on_clear })),
        }
    }

    /// Instantly returns the value from the object if there is any.
    pub fn poll(&self) -> Option<T>
        where T: Clone,
    {
        let inner = self.inner.lock();
        if let UniqueConnecInner::Full { ref value, .. } = &*inner {
            Some(value.clone())
        } else {
            None
        }
    }

    /// Loads the value from the object.
    ///
    /// If the object is empty or has errored earlier, dials the given multiaddress with the
    /// given transport.
    ///
    /// The closure of the `swarm` is expected to call `tie_*()` on the `UniqueConnec`. Failure
    /// to do so will make the `UniqueConnecFuture` produce an error.
    ///
    /// One critical property of this method, is that if a connection incomes and `tie_*` is
    /// called, then it will be returned by the returned future.
    #[inline]
    pub fn dial<S, Du>(&self, swarm: &SwarmController<S>, multiaddr: &Multiaddr,
                              transport: Du) -> UniqueConnecFuture<T>
        where T: Clone + 'static,       // TODO: 'static :-/
              Du: Transport + 'static, // TODO: 'static :-/
              Du::Output: Into<S::Output>,
              S: Clone + MuxedTransport,
    {
        self.dial_inner(swarm, multiaddr, transport, true)
    }

    /// Same as `dial`, except that the future will produce an error if an earlier attempt to dial
    /// has errored.
    #[inline]
    pub fn dial_if_empty<S, Du>(&self, swarm: &SwarmController<S>, multiaddr: &Multiaddr,
                              transport: Du) -> UniqueConnecFuture<T>
        where T: Clone + 'static,       // TODO: 'static :-/
              Du: Transport + 'static, // TODO: 'static :-/
              Du::Output: Into<S::Output>,
              S: Clone + MuxedTransport,
    {
        self.dial_inner(swarm, multiaddr, transport, false)
    }

    /// Inner implementation of `dial_*`.
    fn dial_inner<S, Du>(&self, swarm: &SwarmController<S>, multiaddr: &Multiaddr,
                         transport: Du, dial_if_err: bool) -> UniqueConnecFuture<T>
        where T: Clone + 'static,       // TODO: 'static :-/
              Du: Transport + 'static, // TODO: 'static :-/
              Du::Output: Into<S::Output>,
              S: Clone + MuxedTransport,
    {
        let mut inner = self.inner.lock();
        match &*inner {
            UniqueConnecInner::Empty => (),
            UniqueConnecInner::Errored(_) if dial_if_err => (),
            _ => return UniqueConnecFuture { inner: Arc::downgrade(&self.inner) },
        };

        let weak_inner = Arc::downgrade(&self.inner);

        let (transport, interrupt) = transport.interruptible();
        let dial_fut = swarm.dial_then(multiaddr.clone(), transport,
            move |val: Result<(), IoError>| {
                let inner = match weak_inner.upgrade() {
                    Some(i) => i,
                    None => return val
                };

                let mut inner = inner.lock();
                if let UniqueConnecInner::Full { .. } = *inner {
                    return val;
                }

                let new_val = UniqueConnecInner::Errored(match val {
                    Ok(()) => IoError::new(IoErrorKind::ConnectionRefused,
                        "dialing has succeeded but tie_* hasn't been called"),
                    Err(ref err) => IoError::new(err.kind(), err.to_string()),
                });

                match mem::replace(&mut *inner, new_val) {
                    UniqueConnecInner::Pending { tasks_waiting, .. } => {
                        for task in tasks_waiting {
                            task.1.notify();
                        }
                    },
                    _ => ()
                };

                val
            });

        let dial_fut = dial_fut
            .map_err(|_| IoError::new(IoErrorKind::Other, "multiaddress not supported"))
            .into_future()
            .flatten();

        *inner = UniqueConnecInner::Pending {
            tasks_waiting: Default::default(),
            dial_fut: Box::new(dial_fut),
            interrupt,
        };

        UniqueConnecFuture { inner: Arc::downgrade(&self.inner) }
    }

    /// Puts `value` inside the object.
    /// Additionally, the `UniqueConnec` will be tied to the `until` future. When the future drops
    /// or finishes, the `UniqueConnec` is automatically cleared. If the `UniqueConnec` is cleared
    /// by the user, the future automatically stops.
    /// The returned future is an adjusted version of that same future.
    ///
    /// If the object already contains something, then `until` is dropped and a dummy future that
    /// immediately ends is returned.
    pub fn tie_or_stop<F>(&self, value: T, until: F) -> impl Future<Item = (), Error = F::Error>
        where F: Future<Item = ()>
    {
        self.tie_inner(value, until, false)
    }

    /// Same as `tie_or_stop`, except that is if the object already contains something, then
    /// `until` is returned immediately and can live in parallel.
    pub fn tie_or_passthrough<F>(&self, value: T, until: F) -> impl Future<Item = (), Error = F::Error>
        where F: Future<Item = ()>
    {
        self.tie_inner(value, until, true)
    }

    /// Inner implementation of `tie_*`.
    fn tie_inner<F>(&self, value: T, until: F, pass_through: bool) -> impl Future<Item = (), Error = F::Error>
        where F: Future<Item = ()>
    {
        let mut tasks_to_notify = Default::default();

        let mut inner = self.inner.lock();
        let (on_clear, on_clear_rx) = oneshot::channel();
        match mem::replace(&mut *inner, UniqueConnecInner::Full { value, on_clear }) {
            UniqueConnecInner::Empty => {},
            UniqueConnecInner::Errored(_) => {},
            UniqueConnecInner::Pending { tasks_waiting, .. } => {
                tasks_to_notify = tasks_waiting;
            },
            old @ UniqueConnecInner::Full { .. } => {
                // Keep the old value.
                *inner = old;
                if pass_through {
                    return future::Either::B(future::Either::A(until));
                } else {
                    return future::Either::B(future::Either::B(future::ok(())));
                }
            },
        };
        drop(inner);

        struct Cleaner<T>(Weak<Mutex<UniqueConnecInner<T>>>);
        impl<T> Drop for Cleaner<T> {
            #[inline]
            fn drop(&mut self) {
                if let Some(inner) = self.0.upgrade() {
                    *inner.lock() = UniqueConnecInner::Empty;
                }
            }
        }
        let cleaner = Cleaner(Arc::downgrade(&self.inner));

        // The mutex is unlocked when we notify the pending tasks.
        for task in tasks_to_notify {
            task.1.notify();
        }

        let fut = until
            .select(on_clear_rx.then(|_| Ok(())))
            .map(|((), _)| ())
            .map_err(|(err, _)| err)
            .then(move |val| {
                drop(cleaner);      // Make sure that `cleaner` gets called there.
                val
            });
        future::Either::A(fut)
    }

    /// Clears the content of the object.
    ///
    /// Has no effect if the content is empty or pending.
    /// If the node was full, calling `clear` will stop the future returned by `tie_*`.
    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        match mem::replace(&mut *inner, UniqueConnecInner::Empty) {
            UniqueConnecInner::Empty => {},
            UniqueConnecInner::Errored(_) => {},
            pending @ UniqueConnecInner::Pending { .. } => {
                *inner = pending;
            },
            UniqueConnecInner::Full { on_clear, .. } => {
                // TODO: Should we really replace the `Full` with an `Empty` here? What about
                // letting dropping the future clear the connection automatically? Otherwise
                // it is possible that the user dials before the future gets dropped, in which
                // case the future dropping will set the value to `Empty`. But on the other hand,
                // it is expected that `clear()` is instantaneous and if it is followed with
                // `dial()` then it should dial.
                let _ = on_clear.send(());
            },
        };
    }

    /// Returns the state of the object.
    ///
    /// Note that this can be racy, as the object can be used at the same time. In other words,
    /// the returned value may no longer reflect the actual state.
    pub fn state(&self) -> UniqueConnecState {
        match *self.inner.lock() {
            UniqueConnecInner::Empty => UniqueConnecState::Empty,
            UniqueConnecInner::Errored(_) => UniqueConnecState::Errored,
            UniqueConnecInner::Pending { .. } => UniqueConnecState::Pending,
            UniqueConnecInner::Full { .. } => UniqueConnecState::Full,
        }
    }

    /// Returns true if the object has a pending or active connection. Returns false if the object
    /// is empty or the connection has errored earlier.
    #[inline]
    pub fn is_alive(&self) -> bool {
        match self.state() {
            UniqueConnecState::Empty => false,
            UniqueConnecState::Errored => false,
            UniqueConnecState::Pending => true,
            UniqueConnecState::Full => true,
        }
    }
}

impl<T> Clone for UniqueConnec<T> {
    #[inline]
    fn clone(&self) -> UniqueConnec<T> {
        UniqueConnec {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Default for UniqueConnec<T> {
    #[inline]
    fn default() -> Self {
        UniqueConnec::empty()
    }
}

impl<T> Drop for UniqueConnec<T> {
    fn drop(&mut self) {
        // Notify the waiting futures if we are the last `UniqueConnec`.
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            match *inner.get_mut() {
                UniqueConnecInner::Pending { ref mut tasks_waiting, .. } => {
                    for task in tasks_waiting.drain() {
                        task.1.notify();
                    }
                },
                _ => ()
            }
        }
    }
}

/// Future returned by `UniqueConnec::dial()`.
#[must_use = "futures do nothing unless polled"]
pub struct UniqueConnecFuture<T> {
    inner: Weak<Mutex<UniqueConnecInner<T>>>,
}

impl<T> Future for UniqueConnecFuture<T>
    where T: Clone
{    
    type Item = T;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let inner = match self.inner.upgrade() {
            Some(inner) => inner,
            // All the `UniqueConnec` have been destroyed.
            None => return Err(IoErrorKind::ConnectionAborted.into()),
        };

        let mut inner = inner.lock();
        match mem::replace(&mut *inner, UniqueConnecInner::Empty) {
            UniqueConnecInner::Empty => {
                // This can happen if `tie_*()` is called, and the future expires before the
                // future returned by `dial()` gets polled. This means that the connection has been
                // closed.
                Err(IoErrorKind::ConnectionAborted.into())
            },
            UniqueConnecInner::Pending { mut tasks_waiting, mut dial_fut, interrupt } => {
                match dial_fut.poll() {
                    Ok(Async::Ready(())) => {
                        // This happens if we successfully dialed a remote, but the callback
                        // doesn't call `tie_*`. This can be a logic error by the user,
                        // but could also indicate that the user decided to filter out this
                        // connection for whatever reason.
                        *inner = UniqueConnecInner::Errored(IoErrorKind::ConnectionAborted.into());
                        Err(IoErrorKind::ConnectionAborted.into())
                    },
                    Ok(Async::NotReady) => {
                        static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(0);
                        task_local! {
                            static TASK_ID: usize = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed)
                        }
                        tasks_waiting.insert(TASK_ID.with(|&k| k), task::current());
                        *inner = UniqueConnecInner::Pending { tasks_waiting, dial_fut, interrupt };
                        Ok(Async::NotReady)
                    }
                    Err(err) => {
                        let tr = IoError::new(err.kind(), err.to_string());
                        *inner = UniqueConnecInner::Errored(err);
                        Err(tr)
                    },
                }
            },
            UniqueConnecInner::Full { value, on_clear } => {
                *inner = UniqueConnecInner::Full {
                    value: value.clone(),
                    on_clear
                };
                Ok(Async::Ready(value))
            },
            UniqueConnecInner::Errored(err) => {
                let tr = IoError::new(err.kind(), err.to_string());
                *inner = UniqueConnecInner::Errored(err);
                Err(tr)
            },
        }
    }
}

/// State of a `UniqueConnec`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum UniqueConnecState {
    /// The object is empty.
    Empty,
    /// `dial` has been called and we are waiting for `tie_*` to be called.
    Pending,
    /// `tie_*` has been called.
    Full,
    /// The future returned by the closure of `dial` has errored or has finished before
    /// `tie_*` has been called.
    Errored,
}

#[cfg(test)]
mod tests {
    use futures::{future, sync::oneshot, Future};
    use transport::DeniedTransport;
    use std::io::Error as IoError;
    use std::sync::{Arc, atomic};
    use std::time::Duration;
    use {UniqueConnec, UniqueConnecState};
    use {swarm, transport, Transport};
    use tokio::runtime::current_thread;
    use tokio_timer;

    #[test]
    fn basic_working() {
        // Checks the basic working of the `UniqueConnec`.
        let (tx, rx) = transport::connector();
        let unique_connec = UniqueConnec::empty();
        let unique_connec2 = unique_connec.clone();
        assert_eq!(unique_connec.state(), UniqueConnecState::Empty);

        let (swarm_ctrl, swarm_future) = swarm(rx.with_dummy_muxing(), |_, _| {
            // Note that this handles both the dial and the listen.
            assert!(unique_connec2.is_alive());
            unique_connec2.tie_or_stop(12, future::empty())
        });
        swarm_ctrl.listen_on("/memory".parse().unwrap()).unwrap();

        let dial_success = unique_connec
            .dial(&swarm_ctrl, &"/memory".parse().unwrap(), tx)
            .map(|val| { assert_eq!(val, 12); });
        assert_eq!(unique_connec.state(), UniqueConnecState::Pending);

        let future = dial_success.select(swarm_future).map_err(|(err, _)| err);
        current_thread::Runtime::new().unwrap().block_on(future).unwrap();
        assert_eq!(unique_connec.state(), UniqueConnecState::Full);
    }

    #[test]
    fn invalid_multiaddr_produces_error() {
        // Tests that passing an invalid multiaddress generates an error.
        let unique = UniqueConnec::empty();
        assert_eq!(unique.state(), UniqueConnecState::Empty);
        let unique2 = unique.clone();
        let (swarm_ctrl, _swarm_fut) = swarm(DeniedTransport, |_, _| {
            unique2.tie_or_stop((), future::empty())
        });
        let fut = unique.dial(&swarm_ctrl, &"/ip4/1.2.3.4".parse().unwrap(), DeniedTransport);
        assert!(fut.wait().is_err());
        assert_eq!(unique.state(), UniqueConnecState::Errored);
    }

    #[test]
    fn tie_or_stop_stops() {
        // Tests that `tie_or_stop` destroys additional futures passed to it.
        let (tx, rx) = transport::connector();
        let unique_connec = UniqueConnec::empty();
        let unique_connec2 = unique_connec.clone();

        // This channel is used to detect whether the future has been dropped.
        let (msg_tx, msg_rx) = oneshot::channel();

        let mut num_connec = 0;
        let mut msg_rx = Some(msg_rx);
        let (swarm_ctrl1, swarm_future1) = swarm(rx.with_dummy_muxing(), move |_, _| {
            num_connec += 1;
            if num_connec == 1 {
                unique_connec2.tie_or_stop(12, future::Either::A(future::empty()))
            } else {
                let fut = msg_rx.take().unwrap().map_err(|_| panic!());
                unique_connec2.tie_or_stop(13, future::Either::B(fut))
            }
        });
        swarm_ctrl1.listen_on("/memory".parse().unwrap()).unwrap();

        let (swarm_ctrl2, swarm_future2) = swarm(tx.clone().with_dummy_muxing(), move |_, _| {
            future::empty()
        });

        let dial_success = unique_connec
            .dial(&swarm_ctrl2, &"/memory".parse().unwrap(), tx.clone())
            .map(|val| { assert_eq!(val, 12); })
            .inspect({
                let c = unique_connec.clone();
                move |_| { assert!(c.is_alive()); }
            })
            .and_then(|_| {
                tokio_timer::sleep(Duration::from_secs(1))
                    .map_err(|_| unreachable!())
            })
            .and_then(move |_| {
                swarm_ctrl2.dial("/memory".parse().unwrap(), tx)
                    .unwrap_or_else(|_| panic!())
            })
            .inspect({
                let c = unique_connec.clone();
                move |_| {
                    assert_eq!(c.poll(), Some(12));    // Not 13
                    assert!(msg_tx.send(()).is_err());
                }
            });

        let future = dial_success
            .select(swarm_future2).map(|_| ()).map_err(|(err, _)| err)
            .select(swarm_future1).map(|_| ()).map_err(|(err, _)| err);

        current_thread::Runtime::new().unwrap().block_on(future).unwrap();
        assert!(unique_connec.is_alive());
    }

    #[test]
    fn tie_or_passthrough_passes_through() {
        // Tests that `tie_or_passthrough` doesn't delete additional futures passed to it when
        // it is already full, and doesn't gets its value modified when that happens.
        let (tx, rx) = transport::connector();
        let unique_connec = UniqueConnec::empty();
        let unique_connec2 = unique_connec.clone();

        let mut num = 12;
        let (swarm_ctrl, swarm_future) = swarm(rx.with_dummy_muxing(), move |_, _| {
            // Note that this handles both the dial and the listen.
            let fut = future::empty().then(|_: Result<(), ()>| -> Result<(), IoError> { panic!() });
            num += 1;
            unique_connec2.tie_or_passthrough(num, fut)
        });
        swarm_ctrl.listen_on("/memory".parse().unwrap()).unwrap();

        let dial_success = unique_connec
            .dial(&swarm_ctrl, &"/memory".parse().unwrap(), tx.clone())
            .map(|val| { assert_eq!(val, 13); });

        swarm_ctrl.dial("/memory".parse().unwrap(), tx)
            .unwrap();

        let future = dial_success.select(swarm_future).map_err(|(err, _)| err);
        current_thread::Runtime::new().unwrap().block_on(future).unwrap();
        assert_eq!(unique_connec.poll(), Some(13));
    }

    #[test]
    fn cleared_when_future_drops() {
        // Tests that the `UniqueConnec` gets cleared when the future we associate with it gets
        // destroyed.
        let (tx, rx) = transport::connector();
        let unique_connec = UniqueConnec::empty();
        let unique_connec2 = unique_connec.clone();

        let (msg_tx, msg_rx) = oneshot::channel();
        let mut msg_rx = Some(msg_rx);

        let (swarm_ctrl1, swarm_future1) = swarm(rx.with_dummy_muxing(), move |_, _| {
            future::empty()
        });
        swarm_ctrl1.listen_on("/memory".parse().unwrap()).unwrap();

        let (swarm_ctrl2, swarm_future2) = swarm(tx.clone().with_dummy_muxing(), move |_, _| {
            let fut = msg_rx.take().unwrap().map_err(|_| -> IoError { unreachable!() });
            unique_connec2.tie_or_stop(12, fut)
        });

        let dial_success = unique_connec
            .dial(&swarm_ctrl2, &"/memory".parse().unwrap(), tx)
            .map(|val| { assert_eq!(val, 12); })
            .inspect({
                let c = unique_connec.clone();
                move |_| { assert!(c.is_alive()); }
            })
            .and_then(|_| {
                msg_tx.send(()).unwrap();
                tokio_timer::sleep(Duration::from_secs(1))
                    .map_err(|_| unreachable!())
            })
            .inspect({
                let c = unique_connec.clone();
                move |_| { assert!(!c.is_alive()); }
            });

        let future = dial_success
            .select(swarm_future1).map(|_| ()).map_err(|(err, _)| err)
            .select(swarm_future2).map(|_| ()).map_err(|(err, _)| err);

        current_thread::Runtime::new().unwrap().block_on(future).unwrap();
        assert!(!unique_connec.is_alive());
    }

    #[test]
    fn future_drops_when_cleared() {
        // Tests that the future returned by `tie_or_*` ends when the `UniqueConnec` get cleared.
        let (tx, rx) = transport::connector();
        let unique_connec = UniqueConnec::empty();
        let unique_connec2 = unique_connec.clone();

        let (swarm_ctrl1, swarm_future1) = swarm(rx.with_dummy_muxing(), move |_, _| {
            future::empty()
        });
        swarm_ctrl1.listen_on("/memory".parse().unwrap()).unwrap();

        let finished = Arc::new(atomic::AtomicBool::new(false));
        let finished2 = finished.clone();
        let (swarm_ctrl2, swarm_future2) = swarm(tx.clone().with_dummy_muxing(), move |_, _| {
            let finished2 = finished2.clone();
            unique_connec2.tie_or_stop(12, future::empty()).then(move |v| {
                finished2.store(true, atomic::Ordering::Relaxed);
                v
            })
        });

        let dial_success = unique_connec
            .dial(&swarm_ctrl2, &"/memory".parse().unwrap(), tx)
            .map(|val| { assert_eq!(val, 12); })
            .inspect({
                let c = unique_connec.clone();
                move |_| {
                    assert!(c.is_alive());
                    c.clear();
                    assert!(!c.is_alive());
                }
            })
            .and_then(|_| {
                tokio_timer::sleep(Duration::from_secs(1))
                    .map_err(|_| unreachable!())
            })
            .inspect({
                let c = unique_connec.clone();
                move |_| {
                    assert!(finished.load(atomic::Ordering::Relaxed));
                    assert!(!c.is_alive());
                }
            });

        let future = dial_success
            .select(swarm_future1).map(|_| ()).map_err(|(err, _)| err)
            .select(swarm_future2).map(|_| ()).map_err(|(err, _)| err);

        current_thread::Runtime::new().unwrap().block_on(future).unwrap();
        assert!(!unique_connec.is_alive());
    }

    #[test]
    fn future_drops_when_destroyed() {
        // Tests that the future returned by `tie_or_*` ends when the `UniqueConnec` get dropped.
        let (tx, rx) = transport::connector();
        let unique_connec = UniqueConnec::empty();
        let mut unique_connec2 = Some(unique_connec.clone());

        let (swarm_ctrl1, swarm_future1) = swarm(rx.with_dummy_muxing(), move |_, _| {
            future::empty()
        });
        swarm_ctrl1.listen_on("/memory".parse().unwrap()).unwrap();

        let finished = Arc::new(atomic::AtomicBool::new(false));
        let finished2 = finished.clone();
        let (swarm_ctrl2, swarm_future2) = swarm(tx.clone().with_dummy_muxing(), move |_, _| {
            let finished2 = finished2.clone();
            unique_connec2.take().unwrap().tie_or_stop(12, future::empty()).then(move |v| {
                finished2.store(true, atomic::Ordering::Relaxed);
                v
            })
        });

        let dial_success = unique_connec
            .dial(&swarm_ctrl2, &"/memory".parse().unwrap(), tx)
            .map(|val| { assert_eq!(val, 12); })
            .inspect(move |_| {
                assert!(unique_connec.is_alive());
                drop(unique_connec);
            })
            .and_then(|_| {
                tokio_timer::sleep(Duration::from_secs(1))
                    .map_err(|_| unreachable!())
            })
            .inspect(move |_| {
                assert!(finished.load(atomic::Ordering::Relaxed));
            });

        let future = dial_success
            .select(swarm_future1).map(|_| ()).map_err(|(err, _)| err)
            .select(swarm_future2).map(|_| ()).map_err(|(err, _)| err);

        current_thread::Runtime::new().unwrap().block_on(future).unwrap();
    }

    #[test]
    fn error_if_unique_connec_destroyed_before_future() {
        // Tests that the future returned by `dial` returns an error if the `UniqueConnec` no
        // longer exists.
        let (tx, rx) = transport::connector();

        let (swarm_ctrl, swarm_future) = swarm(rx.with_dummy_muxing(), move |_, _| {
            future::empty()
        });
        swarm_ctrl.listen_on("/memory".parse().unwrap()).unwrap();

        let unique_connec = UniqueConnec::empty();
        let dial_success = unique_connec
            .dial(&swarm_ctrl, &"/memory".parse().unwrap(), tx)
            .then(|val: Result<(), IoError>| {
                assert!(val.is_err());
                Ok(())
            });
        drop(unique_connec);

        let future = dial_success
            .select(swarm_future).map(|_| ()).map_err(|(err, _)| err);
        current_thread::Runtime::new().unwrap().block_on(future).unwrap();
    }

    // TODO: test that dialing is interrupted when UniqueConnec is cleared
    // TODO: test that dialing is interrupted when UniqueConnec is dropped
}
