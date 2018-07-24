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

use futures::{future, sync::oneshot, task, Async, Future, Poll, IntoFuture};
use parking_lot::Mutex;
use {Multiaddr, MuxedTransport, SwarmController, Transport};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::mem;
use std::sync::{Arc, Weak};

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
        tasks_waiting: Vec<task::Task>,
        /// Future that represents when `tie_*` should have been called.
        // TODO: Send + Sync bound is meh
        dial_fut: Box<Future<Item = (), Error = IoError> + Send + Sync>,
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

                *inner = UniqueConnecInner::Errored(match val {
                    Ok(()) => IoError::new(IoErrorKind::ConnectionRefused,
                        "dialing has succeeded but tie_* hasn't been called"),
                    Err(ref err) => IoError::new(err.kind(), err.to_string()),
                });

                val
            });

        let dial_fut = dial_fut
            .map_err(|_| IoError::new(IoErrorKind::Other, "multiaddress not supported"))
            .into_future()
            .flatten();

        *inner = UniqueConnecInner::Pending {
            tasks_waiting: Vec::new(),
            dial_fut: Box::new(dial_fut),
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
        let mut tasks_to_notify = Vec::new();

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

        struct Cleaner<T>(Arc<Mutex<UniqueConnecInner<T>>>);
        impl<T> Drop for Cleaner<T> {
            #[inline]
            fn drop(&mut self) {
                *self.0.lock() = UniqueConnecInner::Empty;
            }
        }
        let cleaner = Cleaner(self.inner.clone());

        // The mutex is unlocked when we notify the pending tasks.
        for task in tasks_to_notify {
            task.notify();
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

/// Future returned by `UniqueConnec::dial()`.
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
            UniqueConnecInner::Pending { mut tasks_waiting, mut dial_fut } => {
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
                        tasks_waiting.push(task::current());
                        *inner = UniqueConnecInner::Pending { tasks_waiting, dial_fut };
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
    use futures::{future, Future};
    use transport::DeniedTransport;
    use {UniqueConnec, UniqueConnecState};
    use swarm;

    #[test]
    fn invalid_multiaddr_produces_error() {
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

    // TODO: more tests
}
