// Copyright 2017 Parity Technologies (UK) Ltd.
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

use futures::{future, stream};
use futures::stream::Then as StreamThen;
use futures::sync::{mpsc, oneshot};
use futures::{Async, Future, Poll, Stream};
use multiaddr::{Protocol, Multiaddr};
use rw_stream_sink::RwStreamSink;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use stdweb::web::TypedArray;
use stdweb::{self, Reference};
use swarm::Transport;
use tokio_io::{AsyncRead, AsyncWrite};

/// Represents the configuration for a websocket transport capability for libp2p.
///
/// This implementation of `Transport` accepts any address that looks like
/// `/ip4/.../tcp/.../ws`, `/ip6/.../tcp/.../ws`, `/dns4/.../ws` or `/dns6/.../ws`, and connect to
/// the corresponding IP and port.
///
/// If the underlying multiaddress uses `/dns4` or `/dns6`, then the domain name will be passed in
/// the headers of the request. This is important is the listener is behind an HTTP proxy.
#[derive(Debug, Clone)]
pub struct BrowserWsConfig;

impl BrowserWsConfig {
    /// Creates a new configuration object for websocket.
    #[inline]
    pub fn new() -> BrowserWsConfig {
        BrowserWsConfig
    }
}

impl Transport for BrowserWsConfig {
    type Output = BrowserWsConn;
    type Listener = stream::Empty<(Self::ListenerUpgrade, Multiaddr), IoError>;
    type ListenerUpgrade = future::Empty<Self::Output, IoError>;
    type Dial = Box<Future<Item = Self::Output, Error = IoError> + Send>;

    #[inline]
    fn listen_on(self, a: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        // Listening is never supported.
        Err((self, a))
    }

    fn dial(self, original_addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        // Making sure we are initialized before we dial. Initialization is protected by a simple
        // boolean static variable, so it's not a problem to call it multiple times and the cost
        // is negligible.
        stdweb::initialize();

        // Tries to interpret the multiaddr, and returns a corresponding `ws://x.x.x.x/` URL (as
        // a string) on success.
        let inner_addr = match multiaddr_to_target(&original_addr) {
            Ok(a) => a,
            Err(_) => return Err((self, original_addr)),
        };

        debug!("Dialing {}", original_addr);

        // Create the JS `WebSocket` object.
        let websocket = {
            let val = js! {
                try {
                    return new WebSocket(@{inner_addr});
                } catch(e) {
                    return false;
                }
            };
            match val.into_reference() {
                Some(ws) => ws,
                None => return Err((self, original_addr)), // `false` was returned by `js!`
            }
        };

        // Create a `message` channel that will be used for both bytes messages and errors, and a
        // `message_cb` used for the `message` event on the WebSocket.
        // `message_tx` is grabbed by `message_cb` and `close_cb`, and `message_rx` is grabbed
        // by `open_cb`.
        let (message_tx, message_rx) = mpsc::unbounded::<Result<Vec<u8>, IoError>>();
        let message_tx = Arc::new(message_tx);
        let mut message_rx = Some(message_rx);
        let message_cb = {
            let message_tx = message_tx.clone();
            move |message_data: Reference| {
                if let Some(buffer) = message_data.downcast::<TypedArray<u8>>() {
                    let _ = message_tx.unbounded_send(Ok(buffer.to_vec()));
                } else {
                    let _ = message_tx.unbounded_send(Err(IoError::new(
                        IoErrorKind::InvalidData,
                        "received ws message of unknown type",
                    )));
                }
            }
        };

        // Create a `open` channel that will be used to communicate the `BrowserWsConn` that represents
        // the open dialing websocket. Also create a `open_cb` callback that will be used for the
        // `open` message of the websocket.
        let (open_tx, open_rx) = oneshot::channel::<Result<BrowserWsConn, IoError>>();
        let open_tx = Arc::new(Mutex::new(Some(open_tx)));
        let websocket_clone = websocket.clone();
        let open_cb = {
            let open_tx = open_tx.clone();
            move || {
                // Note that `open_tx` can be empty (and a panic happens) if the `open` event
                // is triggered twice, or is triggered after the `close` event. We never reuse the
                // same websocket twice, so this is not supposed to happen.
                let tx = open_tx
                    .lock()
                    .unwrap()
                    .take()
                    .expect("the websocket can only open once");
                // `message_rx` can be empty if the `open` event is triggered twice, which again
                // is not supposed to happen.
                let message_rx = message_rx.take().expect("the websocket can only open once");

                // Send a `BrowserWsConn` to the future that was returned by `dial`. Ignoring errors that
                // would happen the future has been dropped by the user.
                let _ = tx.send(Ok(BrowserWsConn {
                    websocket: websocket_clone.clone(),
                    incoming_data: RwStreamSink::new(message_rx.then(|result| {
                        // An `Err` happens here if `message_tx` has been dropped. However
                        // `message_tx` is grabbed by the websocket, which stays alive for as
                        // long as the `BrowserWsConn` is alive.
                        match result {
                            Ok(r) => r,
                            Err(_) => {
                                unreachable!("the message channel outlives the BrowserWsConn")
                            }
                        }
                    })),
                }));
            }
        };

        // Used for the `close` message of the websocket.
        // The websocket can be closed either before or after being opened, so we send an error
        // to both the `open` and `message` channels if that happens.
        let close_cb = move || {
            if let Some(tx) = open_tx.lock().unwrap().take() {
                let _ = tx.send(Err(IoError::new(
                    IoErrorKind::ConnectionRefused,
                    "close event on the websocket",
                )));
            }

            let _ = message_tx.unbounded_send(Err(IoError::new(
                IoErrorKind::ConnectionRefused,
                "close event on the websocket",
            )));
        };

        js! {
            var socket = @{websocket};
            var open_cb = @{open_cb};
            var message_cb = @{message_cb};
            var close_cb = @{close_cb};
            socket.addEventListener("open", function(event) {
                open_cb();
            });
            socket.addEventListener("message", function(event) {
                var reader = new FileReader();
                reader.addEventListener("loadend", function() {
                    var typed = new Uint8Array(reader.result);
                    message_cb(typed);
                });
                reader.readAsArrayBuffer(event.data);
            });
            socket.addEventListener("close", function(event) {
                close_cb();
            });
        };

        Ok(Box::new(open_rx.then(|result| {
            match result {
                Ok(Ok(r)) => Ok(r),
                Ok(Err(e)) => Err(e),
                // `Err` would happen here if `open_tx` is destroyed. `open_tx` is captured by
                // the `WebSocket`, and the `WebSocket` is captured by `open_cb`, which is itself
                // captured by the `WebSocket`. Due to this cyclic dependency, `open_tx` should
                // never be destroyed.
                // TODO: how do we break this cyclic dependency? difficult question
                Err(_) => unreachable!("the sending side will only close when we drop the future"),
            }
        })) as Box<_>)
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        let mut address = Multiaddr::empty();

        let mut iter = server.iter().zip(observed.iter());

        // Use the observed IP address.
        match iter.next() {
            Some((Protocol::Ip4(_), x@Protocol::Ip4(_))) => address.append(x),
            Some((Protocol::Ip6(_), x@Protocol::Ip6(_))) => address.append(x),
            _ => return None
        }

        // Skip over next protocol (assumed to contain port information).
        if iter.next().is_none() {
            return None
        }

        // Check for WS/WSS.
        //
        // Note that it will still work if the server uses WSS while the client uses
        // WS, or vice-versa.
        match iter.next() {
            Some((x@Protocol::Ws, Protocol::Ws)) => address.append(x),
            Some((x@Protocol::Ws, Protocol::Wss)) => address.append(x),
            Some((x@Protocol::Wss, Protocol::Ws)) => address.append(x),
            Some((x@Protocol::Wss, Protocol::Wss)) => address.append(x),
            _ => return None
        }

        // Carry over everything else from the server address.
        for proto in server.iter().skip(3) {
            address.append(proto)
        }

        Some(address)
    }
}

pub struct BrowserWsConn {
    websocket: Reference,
    // Stream of messages that goes through a `RwStreamSink` in order to become a `AsyncRead`.
    incoming_data: RwStreamSink<
        StreamThen<
            mpsc::UnboundedReceiver<Result<Vec<u8>, IoError>>,
            fn(Result<Result<Vec<u8>, IoError>, ()>) -> Result<Vec<u8>, IoError>,
            Result<Vec<u8>, IoError>,
        >,
    >,
}

impl Drop for BrowserWsConn {
    #[inline]
    fn drop(&mut self) {
        // TODO: apparently there's a memory leak related to callbacks?
        js! { @{&self.websocket}.close(); }
    }
}

impl AsyncRead for BrowserWsConn {}

impl Read for BrowserWsConn {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        self.incoming_data.read(buf)
    }
}

impl AsyncWrite for BrowserWsConn {
    #[inline]
    fn shutdown(&mut self) -> Poll<(), IoError> {
        Ok(Async::Ready(()))
    }
}

impl Write for BrowserWsConn {
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        let typed_array = TypedArray::from(buf);

        // `send` can throw if the websocket isn't open (which can happen if it was closed by the
        // remote).
        let returned = js! {
            try {
                @{&self.websocket}.send(@{typed_array.buffer()});
                return true;
            } catch(e) {
                return false;
            }
        };

        match returned {
            stdweb::Value::Bool(true) => Ok(buf.len()),
            stdweb::Value::Bool(false) => Err(IoError::new(
                IoErrorKind::BrokenPipe,
                "websocket has been closed by the remote",
            )),
            _ => unreachable!(),
        }
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        // Everything is always considered flushed.
        Ok(())
    }
}

// Tries to interpret the `Multiaddr` as a `/ipN/.../tcp/.../ws` multiaddress, and if so returns
// the corresponding `ws://.../` URL.
fn multiaddr_to_target(addr: &Multiaddr) -> Result<String, ()> {
    let protocols: Vec<_> = addr.iter().collect();

    if protocols.len() != 3 {
        return Err(());
    }

    match (&protocols[0], &protocols[1], &protocols[2]) {
        (&Protocol::Ip4(ref ip), &Protocol::Tcp(port), &Protocol::Ws) => {
            if ip.is_unspecified() || port == 0 {
                return Err(());
            }
            Ok(format!("ws://{}:{}/", ip, port))
        }
        (&Protocol::Ip6(ref ip), &Protocol::Tcp(port), &Protocol::Ws) => {
            if ip.is_unspecified() || port == 0 {
                return Err(());
            }
            Ok(format!("ws://[{}]:{}/", ip, port))
        }
        (&Protocol::Ip4(ref ip), &Protocol::Tcp(port), &Protocol::Wss) => {
            if ip.is_unspecified() || port == 0 {
                return Err(());
            }
            Ok(format!("wss://{}:{}/", ip, port))
        }
        (&Protocol::Ip6(ref ip), &Protocol::Tcp(port), &Protocol::Wss) => {
            if ip.is_unspecified() || port == 0 {
                return Err(());
            }
            Ok(format!("wss://[{}]:{}/", ip, port))
        }
        (&Protocol::Dns4(ref ns), &Protocol::Tcp(port), &Protocol::Ws) => {
            Ok(format!("ws://{}:{}/", ns, port))
        }
        (&Protocol::Dns6(ref ns), &Protocol::Tcp(port), &Protocol::Ws) => {
            Ok(format!("ws://{}:{}/", ns, port))
        }
        (&Protocol::Dns4(ref ns), &Protocol::Tcp(port), &Protocol::Wss) => {
            Ok(format!("wss://{}:{}/", ns, port))
        }
        (&Protocol::Dns6(ref ns), &Protocol::Tcp(port), &Protocol::Wss) => {
            Ok(format!("wss://{}:{}/", ns, port))
        }
        _ => Err(()),
    }
}

// TODO: write tests (tests are very difficult to write with emscripten)
// - remote refuses connection
// - remote closes connection before we receive
// - remote closes connection before we send
// - remote sends text data instead of binary
