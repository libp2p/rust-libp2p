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

//! Allows using a transport from js-libp2p with rust-libp2p

use futures::{prelude::*, future::FutureResult, sync::mpsc, sync::oneshot};
use libp2p_core::{Multiaddr, Transport, transport::TransportError};
use send_wrapper::SendWrapper;
use std::{error, fmt, io::Write};
use wasm_bindgen::{JsCast, prelude::*};

mod connection;
mod dial;

pub use connection::Connection;
pub use dial::DialFuture;

/// Allows using as a `Transport` a JavaScript object that implements the `js-libp2p-transport`
/// interface.
pub struct JsTransport {
    /// The object that implements `js-libp2p-transport`.
    transport: SendWrapper<JsValue>,

    /// Function that be called with a string in order to build a JavaScript multiaddr, which can
    /// then be passed to the `transport`.
    multiaddr_constructor: SendWrapper<js_sys::Function>,
}

impl Clone for JsTransport {
    fn clone(&self) -> JsTransport {
        JsTransport {
            transport: SendWrapper::new(self.transport.clone()),
            multiaddr_constructor: SendWrapper::new(self.multiaddr_constructor.clone()),
        }
    }
}

impl JsTransport {
    /// Creates an implementation of `Transport` that uses the given JavaScript transport inside.
    ///
    /// Must be passed an object that implements the `js-libp2p-transport` interface, and an
    /// function that, when passed a string, returns a JavaScript multiaddr object.
    pub fn new(transport: JsValue, multiaddr_constructor: JsValue) -> Result<JsTransport, JsErr> {
        let multiaddr_constructor = multiaddr_constructor
            .dyn_into::<js_sys::Function>()
            .map_err(|v| {
                let msg = format!("Expected multiaddr_constructor to be a function, got {:?}", v);
                JsErr::from(JsValue::from_str(&msg))
            })?;

        Ok(JsTransport {
            transport: SendWrapper::new(transport),
            multiaddr_constructor: SendWrapper::new(multiaddr_constructor),
        })
    }

    /// Translates from a Rust `multiaddr` into a JavaScript `multiaddr`.
    fn build_js_multiaddr(&self, addr: Multiaddr) -> Result<JsValue, JsErr> {
        Ok(self.multiaddr_constructor
            .call1(&self.multiaddr_constructor, &JsValue::from_str(&addr.to_string()))?)
    }
}

impl fmt::Debug for JsTransport {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_tuple("JsTransport").finish()
    }
}

impl Transport for JsTransport {
    type Output = Connection;
    type Error = JsErr;
    type Listener = Listener;
    type ListenerUpgrade = FutureResult<Self::Output, Self::Error>;
    type Dial = DialFuture;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>> {
        let js_multiaddr = self.build_js_multiaddr(addr)
            .map_err(|v| TransportError::Other(v.into()))?;

        let create_listener: js_sys::Function = {
            let create_listener = js_sys::Reflect::get(&self.transport, &JsValue::from_str("createListener"))
                .map_err(|v| TransportError::Other(v.into()))?;
            if !create_listener.is_function() {
                // TODO: ?
            }
            // TODO: can panic
            create_listener.into()
        };

        // Spawn the listener with the callback called when we receive an incoming connection.
        let (inc_tx, inc_rx) = mpsc::channel(2);
        let mut inc_tx = Some(inc_tx);
        let incoming_callback = Closure::wrap(Box::new(move |connec| {
            let inc_tx = inc_tx.take().expect("Ready callback called twice");       // TODO: exception instead
            let _ = inc_tx.send(SendWrapper::new(connec));
        }) as Box<FnMut(JsValue)>);
        let listener = create_listener.call2(&create_listener, &JsValue::NULL, incoming_callback.as_ref().unchecked_ref())
            .map_err(|v| TransportError::Other(v.into()))?;

        // Add event listeners for close and error.
        // TODO: ^

        // Start listening. Passes the callback to call when the listener is ready.
        let (ready_tx, ready_rx) = oneshot::channel();
        let mut ready_tx = Some(ready_tx);
        let ready_cb = Closure::wrap(Box::new(move || {
            if let Some(sender) = ready_tx.take() {
                let _ = sender.send(());
            } else {
                wasm_bindgen::throw_str("Listener ready callback has been called multiple times");
            }
        }) as Box<FnMut()>);
        {
            let listen: js_sys::Function = js_sys::Reflect::get(&self.transport, &JsValue::from_str("listen")).unwrap().into();
            listen.call2(&listener, &js_multiaddr, ready_cb.as_ref().unchecked_ref())
                .map_err(|v| TransportError::Other(v.into()))?;
        }

        let listener = Listener {
            listener: Some(SendWrapper::new(listener)),
            listener_ready: ready_rx,
            connection_incoming: inc_rx,
            _incoming_callback: SendWrapper::new(incoming_callback),
            _ready_callback: SendWrapper::new(ready_cb),
        };

        Ok((listener, "/ip4/5.6.7.8/tcp/9".parse().unwrap()))       // TODO: wrong
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // Turn the Rust multiaddr into a JavaScript multiaddr.
        let js_multiaddr = self.build_js_multiaddr(addr).map_err(TransportError::Other)?;
        dial::dial(&self.transport, js_multiaddr).map_err(TransportError::Other)
    }

    fn nat_traversal(&self, _server: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        // TODO: ?
        None
    }
}

/// Error that can be generated by the `JsTransport`.
pub struct JsErr(SendWrapper<JsValue>);

impl From<JsValue> for JsErr {
    fn from(val: JsValue) -> JsErr {
        JsErr(SendWrapper::new(val))
    }
}

impl fmt::Debug for JsErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &*self.0)
    }
}

impl fmt::Display for JsErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &*self.0)
    }
}

impl error::Error for JsErr {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        None
    }
}

/// An active listener.
pub struct Listener {
    /// The object representing the listener.
    listener: Option<SendWrapper<JsValue>>,
    /// Trigger whenever a connection is propagated to the `_incoming_callback`.
    connection_incoming: mpsc::Receiver<SendWrapper<JsValue>>,
    /// Callback called whenever a connection arrives.
    _incoming_callback: SendWrapper<Closure<FnMut(JsValue)>>,
    /// Triggered when the listener is "ready". TODO: useless?
    listener_ready: oneshot::Receiver<()>,
    /// Callback called when the listener is ready.
    _ready_callback: SendWrapper<Closure<FnMut()>>,
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
        Ok(Async::Ready(Some((incoming, "/ip4/1.2.3.4/tcp/5".parse().unwrap()))))
    }
}
