use futures::FutureExt;
use js_sys::Promise;
use std::task::{ready, Context, Poll};
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;

/// Convenient wrapper to poll a promise to completion.
#[derive(Debug)]
pub(crate) struct FusedJsPromise {
    promise: Option<JsFuture>,
}

impl FusedJsPromise {
    /// Creates new uninitialized promise.
    pub(crate) fn new() -> Self {
        FusedJsPromise { promise: None }
    }

    /// Initialize promise if needed and then poll.
    ///
    /// If promise is not initialized then `init` is called to initialize it.
    pub(crate) fn maybe_init_and_poll<F>(
        &mut self,
        cx: &mut Context,
        init: F,
    ) -> Poll<Result<JsValue, JsValue>>
    where
        F: FnOnce() -> Promise,
    {
        if self.promise.is_none() {
            self.promise = Some(JsFuture::from(init()));
        }

        self.poll(cx)
    }

    /// Poll an already initialized promise.
    ///
    /// # Panics
    ///
    /// Panics if promise is not initialized. Use `maybe_init_and_poll` if unsure.
    pub(crate) fn poll(&mut self, cx: &mut Context) -> Poll<Result<JsValue, JsValue>> {
        let val = ready!(self
            .promise
            .as_mut()
            .expect("CachedJsPromise not initialized")
            .poll_unpin(cx));

        // Future finished, drop it
        self.promise.take();

        Poll::Ready(val)
    }

    /// Checks if promise is already running
    pub(crate) fn is_active(&self) -> bool {
        self.promise.is_some()
    }
}
