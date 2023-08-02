use futures::FutureExt;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{ready, Context, Poll, Waker};

pub struct FusedCbFuture<T>(Option<CbFuture<T>>);

impl<T> FusedCbFuture<T> {
    pub(crate) fn new() -> Self {
        Self(None)
    }

    pub(crate) fn maybe_init<F>(&mut self, init: F) -> &mut Self
    where
        F: FnOnce() -> CbFuture<T>,
    {
        if self.0.is_none() {
            self.0 = Some(init());
        }

        self
    }

    /// Checks if future is already running
    pub(crate) fn is_active(&self) -> bool {
        self.0.is_some()
    }
}

impl<T> Future for FusedCbFuture<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let val = ready!(self
            .0
            .as_mut()
            .expect("FusedCbFuture not initialized")
            .poll_unpin(cx));

        // Future finished, drop it
        self.0.take();

        Poll::Ready(val)
    }
}

#[derive(Clone, Debug)]
pub struct CbFuture<T>(Rc<CallbackFutureInner<T>>);

struct CallbackFutureInner<T> {
    waker: Cell<Option<Waker>>,
    result: RefCell<Option<T>>,
}

impl<T> std::fmt::Debug for CallbackFutureInner<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackFutureInner").finish()
    }
}

impl<T> Default for CallbackFutureInner<T> {
    fn default() -> Self {
        Self {
            waker: Cell::new(None),
            result: RefCell::new(None),
        }
    }
}

impl<T> CbFuture<T> {
    /// New Callback Future
    pub(crate) fn new() -> Self {
        Self(Rc::new(CallbackFutureInner::<T>::default()))
    }

    // call this from your callback
    pub(crate) fn publish(&self, result: T) {
        *self.0.result.borrow_mut() = Some(result);
        if let Some(w) = self.0.waker.take() {
            w.wake()
        };
    }

    /// Checks if future is already running
    pub(crate) fn is_active(&self) -> bool {
        // check if self.0.result is_some() without taking it out
        // (i.e. without calling self.0.result.take())
        self.0.result.borrow().is_some()
    }
}

impl<T> Future for CbFuture<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0.result.take() {
            Some(x) => Poll::Ready(x),
            None => {
                self.0.waker.set(Some(cx.waker().clone()));
                Poll::Pending
            }
        }
    }
}

/// Callback Stream
/// Implement futures::Stream for a callback function
#[derive(Clone, Debug)]
pub struct CbStream<T>(Rc<CallbackStreamInner<T>>);

struct CallbackStreamInner<T> {
    waker: Cell<Option<Waker>>,
    result: Cell<Option<T>>,
}

impl<T> std::fmt::Debug for CallbackStreamInner<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackStreamInner").finish()
    }
}

impl<T> Default for CallbackStreamInner<T> {
    fn default() -> Self {
        Self {
            waker: Cell::new(None),
            result: Cell::new(None),
        }
    }
}

impl<T> CbStream<T> {
    /// New Callback Stream
    pub(crate) fn new() -> Self {
        Self(Rc::new(CallbackStreamInner::<T>::default()))
    }

    // call this from your callback
    pub(crate) fn publish(&self, result: T) {
        self.0.result.set(Some(result));
        if let Some(w) = self.0.waker.take() {
            w.wake()
        };
    }
}

impl<T> futures::Stream for CbStream<T> {
    type Item = T;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.0.result.take() {
            Some(x) => Poll::Ready(Some(x)),
            None => {
                self.0.waker.set(Some(cx.waker().clone()));
                Poll::Pending
            }
        }
    }
}
