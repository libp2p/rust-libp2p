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
use futures::{prelude::*, future::FutureResult, sync::mpsc};
use libp2p_core::Multiaddr;
use send_wrapper::SendWrapper;
use std::fmt;
use wasm_bindgen::{JsCast, prelude::*};

/// Start listening on the given JavaScript multiaddress using the given transport.
pub(crate) fn listen_on(transport: &JsValue, js_multiaddr: JsValue)
    -> Result<(Listener, Multiaddr), JsErr>
{
    // Spawn the listener with the callback called when we receive an incoming connection.
    let (connection_incoming, incoming_callback) = {
        let (inc_tx, inc_rx) = mpsc::channel(2);
        let mut inc_tx = Some(inc_tx);
        let cb = Closure::wrap(Box::new(move |connec| {
            let inc_tx = inc_tx.take().expect_throw("Ready callback called twice");
            let _ = inc_tx.send(SendWrapper::new(connec));
        }) as Box<FnMut(JsValue)>);
        (inc_rx, cb)
    };

    // Call `createListener(NULL, incoming_callback)` on the transport.
    let listener = js_sys::Reflect::get(transport, &JsValue::from_str("createListener"))
        .map_err(|err| {
            let msg = format!("Failed to get create_listener from transport: {:?}", err);
            JsErr::from(JsValue::from_str(&msg))
        })?
        .dyn_into::<js_sys::Function>()
        .map_err(|v| {
            let msg = format!("Expected create_listener to be a function, but got {:?}", v);
            JsErr::from(JsValue::from_str(&msg))
        })?
        .call2(transport, &JsValue::NULL, incoming_callback.as_ref().unchecked_ref())?;

    // Add event listeners for close and error.
    // TODO: ^

    // Call `listener.listen(js_multiaddr)`.
    js_sys::Reflect::get(&listener, &JsValue::from_str("listen"))
        .map_err(|err| {
            let msg = format!("Failed to get listen from listener: {:?}", err);
            JsErr::from(JsValue::from_str(&msg))
        })?
        .dyn_into::<js_sys::Function>()
        .map_err(|v| {
            let msg = format!("Expected listen to be a function, but got {:?}", v);
            JsErr::from(JsValue::from_str(&msg))
        })?
        .call1(&listener, &js_multiaddr)?;

    let listener = Listener {
        listener: SendWrapper::new(listener),
        connection_incoming,
        _incoming_callback: SendWrapper::new(incoming_callback),
    };

    Ok((listener, "/ip4/5.6.7.8/tcp/9".parse().unwrap()))       // TODO: wrong
}

/// An active listener.
pub struct Listener {
    /// The object representing the listener.
    listener: SendWrapper<JsValue>,
    /// Trigger whenever a connection is propagated to the `_incoming_callback`.
    connection_incoming: mpsc::Receiver<SendWrapper<JsValue>>,
    /// Callback called whenever a connection arrives.
    _incoming_callback: SendWrapper<Closure<FnMut(JsValue)>>,
}

impl fmt::Debug for Listener {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_tuple("Listener").finish()
    }
}

impl Stream for Listener {
    type Item = (FutureResult<Connection, JsErr>, Multiaddr);
    type Error = JsErr;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Next incoming connection.
        let incoming = match self.connection_incoming.poll() {
            Ok(Async::Ready(Some(v))) => v,
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Ok(Async::Ready(None)) | Err(_) =>
                unreachable!("The sender is in self, so this is never cancelled; QED"),
        };

        let incoming = Connection::from_js_connection(incoming.take()).into_future();
        Ok(Async::Ready(Some((incoming, "/ip4/1.2.3.4/tcp/5".parse().unwrap()))))       // TODO: wrong addr
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        // TODO: the documentation mentions that one can pass a timeout that fires and destroys all
        //       the connections; that seems weird; check what happens

        // Call `listener.close()`.
        let _ = js_sys::Reflect::get(&self.listener, &JsValue::from_str("close"))
            .and_then(|v| v.dyn_into::<js_sys::Function>())
            .and_then(|v| v.call0(&self.listener));
    }
}
