use std::{
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
};

use futures::FutureExt;
use js_sys::Promise;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

/// Convenient wrapper to poll a promise to completion.
///
/// # Panics
///
/// Panics if polled and promise is not initialized. Use `maybe_init` if unsure.
#[derive(Debug)]
pub(crate) struct FusedJsPromise {
    promise: Option<JsFuture>,
}

impl FusedJsPromise {
    /// Creates new uninitialized promise.
    pub(crate) fn new() -> Self {
        FusedJsPromise { promise: None }
    }

    /// Initialize promise if needed
    pub(crate) fn maybe_init<F>(&mut self, init: F) -> &mut Self
    where
        F: FnOnce() -> Promise,
    {
        if self.promise.is_none() {
            self.promise = Some(JsFuture::from(init()));
        }

        self
    }

    /// Checks if promise is already running
    pub(crate) fn is_active(&self) -> bool {
        self.promise.is_some()
    }
}

impl Future for FusedJsPromise {
    type Output = Result<JsValue, JsValue>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let val = ready!(self
            .promise
            .as_mut()
            .expect("FusedJsPromise not initialized")
            .poll_unpin(cx));

        // Future finished, drop it
        self.promise.take();

        Poll::Ready(val)
    }
}
