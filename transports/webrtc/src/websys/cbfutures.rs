use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

#[derive(Clone, Debug)]
pub struct CbFuture<T>(Rc<CallbackFutureInner<T>>);

struct CallbackFutureInner<T> {
    waker: Cell<Option<Waker>>,
    result: Cell<Option<T>>,
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
            result: Cell::new(None),
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
        self.0.result.set(Some(result));
        if let Some(w) = self.0.waker.take() {
            w.wake()
        };
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
