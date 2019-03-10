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
use std::{error, fmt, io, io::Read, io::Write};
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_bindgen::{JsCast, prelude::*};

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
    pub fn new(transport: JsValue, multiaddr_constructor: JsValue) -> JsTransport {
        JsTransport {
            transport: SendWrapper::new(transport),
            multiaddr_constructor: SendWrapper::new(multiaddr_constructor.into()),        // TODO: can panic
        }
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
        let js_multiaddr = self.build_js_multiaddr(addr)
            .map_err(|v| TransportError::Other(v.into()))?;

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
        let connection = js_sys::Reflect::get(&self.transport, &JsValue::from_str("dial"))
            .map_err(|v| TransportError::Other(v.into()))?
            .dyn_into::<js_sys::Function>()
            .map_err(|v| {
                let msg = format!("Expected dial to be a function, but got {:?}", v);
                TransportError::Other(JsErr::from(JsValue::from_str(&msg)))
            })?
            .call3(&self.transport, &js_multiaddr, &JsValue::NULL, callback.as_ref().unchecked_ref())
            .map_err(|v| TransportError::Other(v.into()))?;

        Ok(DialFuture {
            _callback: SendWrapper::new(callback),
            finished,
            connection: Some(SendWrapper::new(connection)),
        })
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

/// An active connection.
pub struct Connection {
    /// The active connection. Expected to conform
    /// to https://github.com/libp2p/interface-connection/.
    connection: SendWrapper<JsValue>,

    /// Data that has been pulled earlier, but we haven't copied to `read` yet.
    pending_data: Vec<u8>,

    /// Channel that is used to transmit back the output of the callback.
    /// The produced buffers can be `None` to signal end-of-file.
    data_inc: mpsc::Receiver<Result<Option<Vec<u8>>, io::Error>>,
    /// Callback to pass when invoking the source. Will transmit its content to `data_inc`.
    /// Conforms to the `cb` as described here: https://www.npmjs.com/package/pull-stream#source-readable-stream-that-produces-values
    source_callback: SendWrapper<Closure<FnMut(JsValue, JsValue)>>,
    /// If true, we invoked the source earlier and are waiting for something to arrive on the
    /// callback.
    has_pending_read: bool,

    /// Channel that "automatically" (ie. without having to poll anything) receives callback
    /// functions that we should invoke back in order to send outgoing data.
    callbacks_in: mpsc::Receiver<SendWrapper<js_sys::Function>>,

    /// When we initialize the connection, we inject this callback in the "sink" part of the
    /// connection. This closure conforms to a `source` as described here: https://www.npmjs.com/package/pull-stream#source-readable-stream-that-produces-values
    /// This closure will then be called by the JavaScript code, and will send the callback passed
    /// to it to `callbacks_in`.
    _writer: SendWrapper<Closure<FnMut(JsValue, JsValue) -> JsValue>>,
}

impl Connection {
    /// Initializes a newly-acquired connection object.
    ///
    /// The `JsValue` should implement the interface of https://github.com/libp2p/interface-connection/.
    fn from_js_connection(connection: JsValue) -> Result<Connection, JsErr> {
        // Let's initialize the writing part.
        // We create an implementation of a "source" (as described here:
        // https://www.npmjs.com/package/pull-stream#source-readable-stream-that-produces-values)
        // and pass it the connection.

        // The `cb_tx`/`cb_rx` channel receives callback functions that we should call when we
        // want to write data.
        let (mut cb_tx, cb_rx) = mpsc::channel(1);

        // This is our "source".
        let writer = Closure::wrap(Box::new(move |end: JsValue, callback: JsValue| -> JsValue {
            let callback = callback.dyn_into::<js_sys::Function>()
                .expect_throw("Passed non-function callback to the source");

            // If `end` is not null, we have to call the `callback` directly with it. This is
            // specified in the documentation.
            if !end.is_null() {
                return callback.call1(&callback, &end).unwrap_throw();
            }

            match cb_tx.start_send(SendWrapper::new(callback)) {
                Ok(AsyncSink::Ready) => (),
                Ok(AsyncSink::NotReady(callback)) => {
                    // The JavaScript API guarantees that the reader (ie. this closure) will only
                    // be called after the callback of the previous invocation has been called.
                    // According to this, we should never have `NotReady`. We signal this error by
                    // calling the callback.
                    let _ = callback.call1(&callback, &JsValue::from_str("Called back the \
                        reader while the previous callback hasn't be called yet."));
                }
                Err(_) => unreachable!("Receiver is stored in self as well; QED")
            };

            JsValue::NULL
        }) as Box<FnMut(JsValue, JsValue) -> JsValue>);

        // Calling `connection.sink()` with our source.
        js_sys::Reflect::get(&connection, &JsValue::from_str("sink"))?
            .dyn_into::<js_sys::Function>()
            .map_err(|v| JsValue::from_str(&format!("Expected function for sink, got {:?}", v)))?
            .call1(&connection, writer.as_ref().unchecked_ref())?;

        // Let's initialize the reading part.
        // We create a callback that we will pass to the connection whenever we want to read from
        // it. The connection will later call back this callback with a buffer.
        // The JavaScript code is only allowed to call the callback again after we've processed
        // the value, and therefore a channel capacity of 1 looks it would be enough. However the
        // JavaScript is also allowed to call the callback to interrupt the stream, so we need a
        // capacity of 2.
        let (mut inc_tx, inc_rx) = mpsc::channel(2);
        let source_callback = Closure::wrap(Box::new(move |end: JsValue, data: JsValue| {
            let to_send = if end.is_null() {
                // If `end` is null, then `data` is valid.
                let data = js_sys::Uint8Array::new(&data);
                let mut buf = vec![0; data.length() as usize];
                data.copy_to(&mut buf[..]);
                Ok(Some(buf))
            } else if end == true {
                // EOF. `data` is not valid.
                Ok(None)
            } else {
                // Error. `data` is not valid.
                Err(io::Error::new(io::ErrorKind::Other, format!("{:?}", end)))
            };

            match inc_tx.try_send(to_send) {
                Ok(()) => (),
                Err(err) => {
                    debug_assert!(err.is_full());
                    wasm_bindgen::throw_str("pull-stream called the reader callback multiple times \
                                             in a row");
                },
            }
        }) as Box<dyn FnMut(JsValue, JsValue)>);

        Ok(Connection {
            connection: SendWrapper::new(connection),
            pending_data: Vec::new(),
            data_inc: inc_rx,
            source_callback: SendWrapper::new(source_callback),
            has_pending_read: false,
            callbacks_in: cb_rx,
            _writer: SendWrapper::new(writer),
        })
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_tuple("Connection").finish()
    }
}

impl AsyncRead for Connection {
}

impl Read for Connection {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        loop {
            // Starting with processing the pending data.
            if !self.pending_data.is_empty() {
                if buf.len() <= self.pending_data.len() {
                    buf.copy_from_slice(&self.pending_data[..buf.len()]);
                    self.pending_data = self.pending_data.split_off(buf.len());
                    return Ok(buf.len());
                } else {
                    let len = self.pending_data.len();
                    buf[..len].copy_from_slice(&self.pending_data);
                    self.pending_data.clear();
                    return Ok(len);
                }
            }

            // Process the channel of incoming data.
            match self.data_inc.poll() {
                Ok(Async::Ready(Some(Ok(Some(data))))) => {
                    debug_assert!(self.pending_data.is_empty());
                    self.pending_data = data;
                },
                Ok(Async::Ready(Some(Err(err)))) => {
                    // An error has been propagated from the JavaScript side.
                    return Err(err);
                },
                Ok(Async::Ready(Some(Ok(None)))) => {
                    // EOF
                    return Ok(0);
                },
                Ok(Async::NotReady) => (),
                Err(_) | Ok(Async::Ready(None)) => {
                    unreachable!("Sender is contained in source_callback, and therefore \
                        never closes")
                },
            }

            // If we are already waiting for the source, nothing more we can do.
            if self.has_pending_read {
                return Err(io::ErrorKind::WouldBlock.into());
            }

            // Start reading.
            let source: js_sys::Function = js_sys::Reflect::get(&self.connection, &JsValue::from_str("source")).unwrap().into();     // TODO: don't panic
            source.call2(&self.connection, &JsValue::NULL, self.source_callback.as_ref().unchecked_ref()).unwrap();   // TODO: don't panic
            self.has_pending_read = true;
            return Err(io::ErrorKind::WouldBlock.into());
        }
    }
}

impl AsyncWrite for Connection {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        // Grab the next available callback when the sync is ready.
        let cb = match self.callbacks_in.poll() {
            Ok(Async::Ready(Some(callback))) => callback,
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(_) | Ok(Async::Ready(None)) => {
                unreachable!("Sender is contained in writer, and therefore never closes")
            },
        };

        // We indicate EOF by sending `true`.
        cb.call1(&cb, &JsValue::from_bool(true));
        Ok(Async::Ready(()))
    }
}

impl Write for Connection {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        // Grab the next available callback when the sync is ready.
        let cb = match self.callbacks_in.poll() {
            Ok(Async::Ready(Some(callback))) => callback,
            Ok(Async::NotReady) => return Err(io::ErrorKind::WouldBlock.into()),
            Err(_) | Ok(Async::Ready(None)) => {
                unreachable!("Sender is contained in writer, and therefore never closes")
            },
        };

        // Turn the input buffer into an `ArrayBuffer`.
        let array_buf: js_sys::ArrayBuffer = {
            // This unsafe is here because the lifetime of `other_public_key` must not outlive the
            // `tmp_view`. This is guaranteed by the fact that we clone this array right below.
            let tmp_view = unsafe { js_sys::Uint8Array::view(buf) };
            js_sys::Uint8Array::new(tmp_view.as_ref()).buffer()
        };

        // `cb(null, data)` as described here: https://www.npmjs.com/package/pull-stream#source-readable-stream-that-produces-values
        // Note that the type of data actually accepted is not documented. We magically assume that
        // `ArrayBuffer` will work.
        cb.call2(&cb, &JsValue::NULL, &array_buf).unwrap();      // TODO: no
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        // Everything is always considered flushed.
        Ok(())
    }
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
