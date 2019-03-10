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

use crate::JsErr;
use futures::{prelude::*, sync::mpsc};
use send_wrapper::SendWrapper;
use std::{fmt, io, io::Read, io::Write};
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_bindgen::{JsCast, prelude::*};

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
    /// Takes ownership of a newly-created JavaScript connection object and initializes it.
    ///
    /// The `JsValue` should implement the interface of
    /// https://github.com/libp2p/interface-connection/.
    pub(crate) fn from_js_connection(connection: JsValue) -> Result<Connection, JsErr> {
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
                // TODO: clear the content of cb_tx? the JavaScript doc doesn't cover half of the
                // corner cases that we have to cover
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
        //
        // We create a callback. We don't use it immediately, but we will pass it to the connection
        // whenever we want to read from it. The connection will later call back this callback with
        // a buffer.
        //
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
                    self.has_pending_read = false;
                    debug_assert!(self.pending_data.is_empty());
                    self.pending_data = data;
                    continue;
                },
                Ok(Async::Ready(Some(Err(err)))) => {
                    // An error has been propagated from the JavaScript side.
                    self.has_pending_read = false;
                    return Err(err);
                },
                Ok(Async::Ready(Some(Ok(None)))) => {
                    // EOF
                    self.has_pending_read = false;
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

            // Start reading by calling `connection.source(callback)`.
            js_sys::Reflect::get(&self.connection, &JsValue::from_str("source"))
                .map_err(|err| {
                    let msg = format!("Failed to get source() field on connection: {:?}", err);
                    io::Error::new(io::ErrorKind::Other, msg)
                })?
                .dyn_into::<js_sys::Function>()
                .map_err(|val| {
                    let msg = format!("Source() on connection isn't a method: {:?}", val);
                    io::Error::new(io::ErrorKind::Other, msg)
                })?
                .call2(&self.connection, &JsValue::NULL, self.source_callback.as_ref().unchecked_ref())
                .map_err(|err| {
                    let msg = format!("Failed to call source() method on connection: {:?}", err);
                    io::Error::new(io::ErrorKind::Other, msg)
                })?;
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
        cb.call1(&cb, &JsValue::from_bool(true))
            .map_err(|err| {
                let msg = format!("Failed to call callback with EOF signal: {:?}", err);
                io::Error::new(io::ErrorKind::Other, msg)
            })?;
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
            // `tmp_view`. This is guaranteed by the fact that we clone this array the line right
            // after. See https://github.com/rustwasm/wasm-bindgen/issues/1303.
            let tmp_view = unsafe { js_sys::Uint8Array::view(buf) };
            js_sys::Uint8Array::new(tmp_view.as_ref()).buffer()
        };

        // `cb(null, data)` as described here: https://www.npmjs.com/package/pull-stream#source-readable-stream-that-produces-values
        // Note that the type of data actually accepted is not documented. We magically assume that
        // `ArrayBuffer` will work.
        cb.call2(&cb, &JsValue::NULL, &array_buf)
            .map_err(|err| {
                let msg = format!("Failed to call write callback: {:?}", err);
                io::Error::new(io::ErrorKind::Other, msg)
            })?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        // Everything is always considered flushed.
        Ok(())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // TODO: cancel the callbacks? again, not sure how to do that
    }
}
