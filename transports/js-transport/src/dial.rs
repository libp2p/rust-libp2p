// Copyright 2019 Parity Technologies (UK) Ltd.
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

use crate::{Connection, JsErr};
use futures::{prelude::*, sync::oneshot};
use send_wrapper::SendWrapper;
use std::fmt;
use wasm_bindgen::{JsCast, prelude::*};

/// Start dialing the given multiaddr with the given transport.
pub(crate) fn dial(transport: &JsValue, js_multiaddr: JsValue)
    -> Result<DialFuture, JsErr>
{
    // The `dial` function directly returns the connection, but also expects a callback that
    // will be passed either NULL on success, or an error.
    let (callback, finished) = {
        let (tx, rx) = oneshot::channel();
        let mut tx = Some(tx);
        let callback = Closure::wrap(Box::new(move |result| {
            // When we closure is called, we move out of `tx`. If the closure is called a
            // second time, `tx` will be empty.
            if let Some(sender) = tx.take() {
                let _ = sender.send(SendWrapper::new(result));
            } else {
                wasm_bindgen::throw_str("Dialing callback has been called multiple times");
            }
        }) as Box<FnMut(JsValue)>);
        (callback, rx)
    };

    // Call `dial(multiaddr, NULL, callback)` on our JavaScript transport.
    let connection = js_sys::Reflect::get(transport, &JsValue::from_str("dial"))?
        .dyn_into::<js_sys::Function>()
        .map_err(|v| {
            let msg = format!("Expected dial to be a function, but got {:?}", v);
            JsErr::from(JsValue::from_str(&msg))
        })?
        .call3(transport, &js_multiaddr, &JsValue::NULL, callback.as_ref().unchecked_ref())?;

    Ok(DialFuture {
        _callback: SendWrapper::new(callback),
        finished,
        connection: Some(SendWrapper::new(connection)),
    })
}

/// Future for establishing the connection.
pub struct DialFuture {
    /// Callback called by the JavaScript. We need to keep it alive.
    // TODO: what to do if we drop before finished?
    _callback: SendWrapper<Closure<FnMut(JsValue)>>,
    /// Channel that receives the output of the closure (null on success, something else on error).
    finished: oneshot::Receiver<SendWrapper<JsValue>>,
    /// Connection waiting to be established. `None` if the future has succeeded and we have moved
    /// it out.
    connection: Option<SendWrapper<JsValue>>,
}

impl fmt::Debug for DialFuture {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_tuple("DialFuture").finish()
    }
}

impl Future for DialFuture {
    type Item = Connection;
    type Error = JsErr;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let value = match self.finished.poll() {
            Ok(Async::Ready(v)) => v,
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(_) => unreachable!("The sender is in self, so this is never cancelled; QED"),
        };

        // Value contains the potential error that happened.
        if !value.is_null() {
            return Err(JsErr(value));
        }

        // Success!
        let connection = self.connection.take().expect("Future has already succeeded in the past");
        Ok(Async::Ready(Connection::from_js_connection(connection.take())?))
    }
}
