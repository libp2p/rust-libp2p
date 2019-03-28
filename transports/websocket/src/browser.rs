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

use libp2p_core as swarm;
use log::debug;
use futures::{future, stream, try_ready};
use futures::stream::Then as StreamThen;
use futures::sync::{mpsc, oneshot};
use futures::{Async, Future, Poll, Stream};
use multiaddr::{Protocol, Multiaddr};
use rw_stream_sink::RwStreamSink;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use swarm::{Transport, transport::TransportError};
use tokio_io::{AsyncRead, AsyncWrite};
use wasm_bindgen::{prelude::*, JsCast};

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
    type Error = io::Error;   // TODO: better error type?
    type Listener = stream::Empty<(Self::ListenerUpgrade, Multiaddr), io::Error>;
    type ListenerUpgrade = future::Empty<Self::Output, io::Error>;
    type Dial = BrowserWsConnFuture;

    #[inline]
    fn listen_on(self, a: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>> {
        // Listening is never supported.
        Err(TransportError::MultiaddrNotSupported(a))
    }

    fn dial(self, original_addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // Tries to interpret the multiaddr, and returns a corresponding `ws://x.x.x.x/` URL (as
        // a string) on success.
        let inner_addr = match multiaddr_to_target(&original_addr) {
            Ok(a) => a,
            Err(_) => return Err(TransportError::MultiaddrNotSupported(original_addr)),
        };

        debug!("Dialing {}", original_addr);

        let websocket = web_sys::WebSocket::new(&inner_addr).unwrap();      // TODO: don't unwrap

        let (tx, rx) = mpsc::unbounded();

        let open_cb = {
            let tx = tx.clone();
            Closure::wrap(Box::new(move |event: web_sys::Event| {
                let _ = tx.send(InnerEvent::Open(event));
            }) as Box<FnMut(web_sys::Event)>)
        };

        let message_cb = {
            let tx = tx.clone();
            Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
                let _ = tx.send(InnerEvent::Message(event));
            }) as Box<FnMut(web_sys::MessageEvent)>)
        };

        let close_cb = {
            let tx = tx.clone();
                Closure::wrap(Box::new(move |event: web_sys::CloseEvent| {
                let _ = tx.send(InnerEvent::Close(event));
            }) as Box<FnMut(web_sys::CloseEvent)>)
        };

        websocket.set_onopen(Some(open_cb.as_ref().unchecked_ref()));
        websocket.set_onmessage(Some(message_cb.as_ref().unchecked_ref()));
        websocket.set_onclose(Some(close_cb.as_ref().unchecked_ref()));

        Ok(BrowserWsConnFuture {
            inner: Some(BrowserWsConn {
                websocket,
                messages: rx,
                messages_tx: tx,
                open_cb,
                message_cb,
                close_cb,
                file_readers: Vec::new(),
                pending_read: Vec::new(),
            }),
        })
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

/// Internal event communicate on the channel.
enum InnerEvent {
    Open(web_sys::Event),
    Array(js_sys::Uint8Array),
    Message(web_sys::MessageEvent),
    Close(web_sys::CloseEvent),
}

/// Future for creating a WebSocket connection.
pub struct BrowserWsConnFuture {
    inner: Option<BrowserWsConn>,
}

impl Future for BrowserWsConnFuture {
    type Item = BrowserWsConn;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let event = try_ready!(self.inner.as_mut().unwrap().messages.poll().map_err(|_| -> io::Error { unreachable!() }));
        match event {
            Some(InnerEvent::Open(_)) => Ok(Async::Ready(self.inner.take().unwrap())),
            Some(InnerEvent::Close(_)) => Err(io::ErrorKind::ConnectionAborted.into()),
            Some(InnerEvent::Message(_)) | Some(InnerEvent::Array(_)) | None => panic!(),
        }
    }
}

/// Active WebSocket connection.
pub struct BrowserWsConn {
    websocket: web_sys::WebSocket,
    messages_tx: mpsc::UnboundedSender<InnerEvent>,
    messages: mpsc::UnboundedReceiver<InnerEvent>,
    open_cb: Closure<FnMut(web_sys::Event)>,
    message_cb: Closure<FnMut(web_sys::MessageEvent)>,
    close_cb: Closure<FnMut(web_sys::CloseEvent)>,
    file_readers: Vec<Closure<FnMut(web_sys::Event)>>,      // TODO: leaky
    /// Buffer of data that has been read from the WebSocket and waiting for .
    pending_read: Vec<u8>,
}

// TODO: ?
unsafe impl Send for BrowserWsConn {}
// TODO: ?
unsafe impl Sync for BrowserWsConn {}

impl Drop for BrowserWsConn {
    #[inline]
    fn drop(&mut self) {
        web_sys::console::log_1(&JsValue::from_str("dropping"));
        self.websocket.set_onopen(None);
        self.websocket.set_onmessage(None);
        self.websocket.set_onclose(None);
        let _ = self.websocket.close();     // TODO: don't close if already closed
    }
}

impl AsyncRead for BrowserWsConn {}

impl Read for BrowserWsConn {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        loop {
            if !self.pending_read.is_empty() {
                if buf.len() <= self.pending_read.len() {
                    buf.copy_from_slice(&self.pending_read[..buf.len()]);
                    self.pending_read = self.pending_read.split_off(buf.len());
                    return Ok(buf.len());
                } else {
                    let len = self.pending_read.len();
                    buf[..len].copy_from_slice(&self.pending_read);
                    self.pending_read.clear();
                    return Ok(len);
                }
            }

            match self.messages.poll() {
                Ok(Async::Ready(Some(InnerEvent::Array(arr)))) => {
                    let arr = js_sys::Uint8Array::new(&arr);
                    let arr_len = arr.length() as usize;
                    if buf.len() >= arr_len && arr_len != 0 {
                        arr.copy_to(&mut buf[..arr_len]);
                        return Ok(arr_len);
                    } else {
                        debug_assert!(self.pending_read.is_empty());
                        self.pending_read.resize(arr_len, 0);
                        arr.copy_to(&mut self.pending_read[..arr_len]);
                        continue;
                    }
                },

                Ok(Async::Ready(Some(InnerEvent::Message(message)))) => {
                    if let Some(blob) = message.data().dyn_ref::<web_sys::Blob>() {
                        let reader = web_sys::FileReader::new().unwrap();
                        let reader_clone = reader.clone();
                        let cb = {
                            let tx = self.messages_tx.clone();
                            Closure::wrap(Box::new(move |event: web_sys::Event| {
                                let result = reader_clone.result().unwrap();
                                let _ = tx.send(InnerEvent::Array(result.into()));
                            }) as Box<FnMut(web_sys::Event)>)
                        };
                        reader.set_onload(Some(cb.as_ref().unchecked_ref()));
                        reader.read_as_array_buffer(blob).unwrap();
                        self.file_readers.push(cb);
                        continue;

                    } else if let Some(arr) = message.data().as_string() { // TODO:
                        panic!()

                    } else { // TODO:
                        web_sys::console::log_1(&message.data());
                        panic!()
                    }
                },
                Ok(Async::Ready(Some(InnerEvent::Close(_)))) => {
                    return Err(io::ErrorKind::ConnectionAborted.into())
                },
                Ok(Async::Ready(Some(InnerEvent::Open(_)))) | Ok(Async::Ready(None)) | Err(_) => panic!(),
                Ok(Async::NotReady) => return Err(io::ErrorKind::WouldBlock.into()),
            }
        }
    }
}

impl AsyncWrite for BrowserWsConn {
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        web_sys::console::log_1(&JsValue::from_str("shutting down"));
        let _ = self.websocket.close();     // TODO: don't close if already closed
        loop {
            let event = try_ready!(self.messages.poll().map_err(|_| -> io::Error { unreachable!() }));
            match event {
                Some(InnerEvent::Open(_)) => (),
                Some(InnerEvent::Close(_)) => return Ok(Async::Ready(())),
                Some(InnerEvent::Message(_)) | Some(InnerEvent::Array(_)) | None => (),
            }
        }
    }
}

impl Write for BrowserWsConn {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        let mut buf2 = buf.to_vec();
        web_sys::console::log_1(&JsValue::from_str(&format!("write {:?} bytes", buf2.len())));
        self.websocket.send_with_u8_array(&mut buf2).unwrap();      // TODO: error handling
        Ok(buf.len())
    }

    #[inline]
    fn flush(&mut self) -> Result<(), io::Error> {
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
