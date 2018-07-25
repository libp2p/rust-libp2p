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
        /// Future that represents when `set_until` should have been called.
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
    /// If the object is empty, dials the given multiaddress with the given transport.
    ///
    /// The closure of the `swarm` is expected to call `set_until()` on the `UniqueConnec`. Failure
    /// to do so will make the `UniqueConnecFuture` produce an error.
    pub fn get_or_dial<S, Du>(&self, swarm: &SwarmController<S>, multiaddr: &Multiaddr,
                              transport: Du) -> UniqueConnecFuture<T>
        where T: Clone,
              Du: Transport + 'static, // TODO: 'static :-/
              Du::Output: Into<S::Output>,
              S: Clone + MuxedTransport,
    {
        self.get(|| {
            swarm.dial(multiaddr.clone(), transport)
                .map_err(|_| IoError::new(IoErrorKind::Other, "multiaddress not supported"))
                .into_future()
                .flatten()
        })
    }

    /// Loads the value from the object.
    ///
    /// If the object is empty, calls the closure. The closure should return a future that
    /// should be signaled after `set_until` has been called. If the future produces an error,
    /// then the object will empty itself again and the `UniqueConnecFuture` will return an error.
    /// If the future is finished and `set_until` hasn't been called, then the `UniqueConnecFuture`
    /// will return an error.
    pub fn get<F, Fut>(&self, or: F) -> UniqueConnecFuture<T>
        where F: FnOnce() -> Fut,
              T: Clone,
              Fut: IntoFuture<Item = (), Error = IoError>,
              Fut::Future: Send + Sync + 'static, // TODO: 'static :-/
    {
        match &*self.inner.lock() {
            UniqueConnecInner::Empty => (),
            _ => return UniqueConnecFuture { inner: Arc::downgrade(&self.inner) },
        };

        // The mutex is unlocked when we call `or`, in order to avoid potential deadlocks.
        let dial_fut = or().into_future();

        let mut inner = self.inner.lock();
        // Since we unlocked the mutex, it's possible that the object was filled in the meanwhile.
        // Therefore we check again whether it's still `Empty`.
        if let UniqueConnecInner::Empty = &mut *inner {
            *inner = UniqueConnecInner::Pending {
                tasks_waiting: Vec::new(),
                dial_fut: Box::new(dial_fut),
            };
        }

        UniqueConnecFuture { inner: Arc::downgrade(&self.inner) }
    }

    /// Puts `value` inside the object. The second parameter is a future whose completion will
    /// clear up the content. Returns an adjusted version of that same future.
    ///
    /// If `clear()` is called, the returned future will automatically complete with an error.
    ///
    /// Has no effect if the object already contains something.
    pub fn set_until<F>(&self, value: T, until: F) -> impl Future<Item = (), Error = F::Error>
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
                return future::Either::B(until);
            },
        };
        drop(inner);

        // The mutex is unlocked when we notify the pending tasks.
        for task in tasks_to_notify {
            task.notify();
        }

        let inner = self.inner.clone();
        let fut = until
            .select(on_clear_rx.then(|_| Ok(())))
            .map(|((), _)| ())
            .map_err(|(err, _)| err)
            .then(move |val| {
                *inner.lock() = UniqueConnecInner::Empty;
                val
            });
        future::Either::A(fut)
    }

    /// Clears the content of the object.
    ///
    /// Has no effect if the content is empty or pending.
    /// If the node was full, calling `clear` will stop the future returned by `set_until`.
    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        match mem::replace(&mut *inner, UniqueConnecInner::Empty) {
            UniqueConnecInner::Empty => {},
            UniqueConnecInner::Errored(_) => {},
            pending @ UniqueConnecInner::Pending { .. } => {
                *inner = pending;
            },
            UniqueConnecInner::Full { on_clear, .. } => {
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

/// Future returned by `UniqueConnec::get()`.
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
                // This can happen if `set_until()` is called, and the future expires before the
                // future returned by `get()` gets polled. This means that the connection has been
                // closed.
                Err(IoErrorKind::ConnectionAborted.into())
            },
            UniqueConnecInner::Pending { mut tasks_waiting, mut dial_fut } => {
                match dial_fut.poll() {
                    Ok(Async::Ready(())) => {
                        // This happens if we successfully dialed a remote, but the callback
                        // doesn't call `set_until`. This can be a logic error by the user,
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
                        let tr = IoError::new(IoErrorKind::ConnectionAborted, err.to_string());
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
                let tr = IoError::new(IoErrorKind::ConnectionAborted, err.to_string());
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
    /// `get_*` has been called and we are waiting for `set_until` to be called.
    Pending,
    /// `set_until` has been called.
    Full,
    /// The future returned by the closure of `get_*` has errored or has finished before
    /// `set_until` has been called.
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
            unique2.set_until((), future::empty())
        });
        let fut = unique.get_or_dial(&swarm_ctrl, &"/ip4/1.2.3.4".parse().unwrap(),
                                     DeniedTransport);
        assert!(fut.wait().is_err());
        assert_eq!(unique.state(), UniqueConnecState::Errored);
    }

    // TODO: more tests
}
