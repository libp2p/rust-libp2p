// Copyright (C) 2023 Vince Vasta
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! Libp2p websocket transports built on [Websys](https://rustwasm.github.io/wasm-bindgen/web-sys/index.html).
use futures::{future::Ready, io, prelude::*};
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, TransportError, TransportEvent},
};
use parking_lot::Mutex;
use send_wrapper::SendWrapper;
use wasm_bindgen::{prelude::*, JsCast};
use web_sys::{MessageEvent, WebSocket};

use std::{
    collections::VecDeque,
    pin::Pin,
    sync::Arc,
    task::Poll,
    task::{Context, Waker},
};

/// A Websocket transport that can be used in a Wasm client.
///
/// ## Example
///
/// To create an authenticated transport instance with Noise protocol and Yamux:
///
/// ```no_run
/// # use libp2p_core::{upgrade::Version, Transport};
/// # use libp2p_identity::Keypair;
/// # use libp2p_yamux::YamuxConfig;
/// # use libp2p_noise::NoiseAuthenticated;
/// let local_key = Keypair::generate_ed25519();
/// let transport = libp2p_websys_websocket::Transport::default()
///     .upgrade(Version::V1)
///     .authenticate(NoiseAuthenticated::xx(&local_key).unwrap())
///     .multiplex(YamuxConfig::default())
///     .boxed();
/// ```
///
#[derive(Default)]
pub struct Transport;

impl libp2p_core::Transport for Transport {
    type Output = Connection;
    type Error = Error;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(&mut self, _addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        Err(TransportError::Other(Error::NotSupported))
    }

    fn remove_listener(&mut self, _id: ListenerId) -> bool {
        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let ws_url = if let Some(url) = websocket_url(&addr) {
            url
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        Ok(async move {
            let socket = match WebSocket::new(&ws_url) {
                Ok(ws) => ws,
                Err(_) => return Err(Error::JsError(format!("Invalid websocket url: {ws_url}"))),
            };

            Ok(Connection::new(socket))
        }
        .boxed())
    }

    fn dial_as_listener(
        &mut self,
        _addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        Err(TransportError::Other(Error::NotSupported))
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> std::task::Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
    }

    fn address_translation(&self, _listen: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        None
    }
}

// Try to convert Multiaddr to a Websocket url.
fn websocket_url(addr: &Multiaddr) -> Option<String> {
    let mut protocols = addr.iter();
    let host_port = match (protocols.next(), protocols.next()) {
        (Some(Protocol::Ip4(ip)), Some(Protocol::Tcp(port))) => {
            format!("{ip}:{port}")
        }
        (Some(Protocol::Ip6(ip)), Some(Protocol::Tcp(port))) => {
            format!("[{ip}]:{port}")
        }
        (Some(Protocol::Dns(h)), Some(Protocol::Tcp(port)))
        | (Some(Protocol::Dns4(h)), Some(Protocol::Tcp(port)))
        | (Some(Protocol::Dns6(h)), Some(Protocol::Tcp(port)))
        | (Some(Protocol::Dnsaddr(h)), Some(Protocol::Tcp(port))) => {
            format!("{}:{}", &h, port)
        }
        _ => return None,
    };

    let (scheme, wspath) = match protocols.next() {
        Some(Protocol::Ws(path)) => ("ws", path.into_owned()),
        Some(Protocol::Wss(path)) => ("wss", path.into_owned()),
        _ => return None,
    };

    Some(format!("{scheme}://{host_port}{wspath}"))
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("js function error {0}")]
    JsError(String),
    #[error("operation not supported")]
    NotSupported,
}

/// A Websocket connection created by the [`Transport`].
pub struct Connection {
    shared: Arc<Mutex<Shared>>,
}

struct Shared {
    opened: bool,
    closed: bool,
    error: bool,
    data: VecDeque<u8>,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
    socket: SendWrapper<WebSocket>,
    closures: Option<SendWrapper<Closures>>,
}

impl Shared {
    fn wake(&self) {
        if let Some(waker) = &self.read_waker {
            waker.wake_by_ref();
        }

        if let Some(waker) = &self.write_waker {
            waker.wake_by_ref();
        }
    }
}

type Closures = (
    Closure<dyn FnMut()>,
    Closure<dyn FnMut(MessageEvent)>,
    Closure<dyn FnMut(web_sys::Event)>,
    Closure<dyn FnMut(web_sys::CloseEvent)>,
);

impl Connection {
    fn new(socket: WebSocket) -> Self {
        socket.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let shared = Arc::new(Mutex::new(Shared {
            opened: false,
            closed: false,
            error: false,
            data: VecDeque::with_capacity(1 << 16),
            read_waker: None,
            write_waker: None,
            socket: SendWrapper::new(socket.clone()),
            closures: None,
        }));

        let open_callback = Closure::<dyn FnMut()>::new({
            let weak_shared = Arc::downgrade(&shared);
            move || {
                if let Some(shared) = weak_shared.upgrade() {
                    let mut locked = shared.lock();
                    locked.opened = true;
                    locked.wake();
                }
            }
        });
        socket.set_onopen(Some(open_callback.as_ref().unchecked_ref()));

        let message_callback = Closure::<dyn FnMut(_)>::new({
            let weak_shared = Arc::downgrade(&shared);
            move |e: MessageEvent| {
                if let Ok(abuf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                    if let Some(shared) = weak_shared.upgrade() {
                        let mut locked = shared.lock();
                        let bytes = js_sys::Uint8Array::new(&abuf).to_vec();
                        locked.data.extend(bytes.into_iter());
                        locked.wake();
                    }
                } else {
                    debug_assert!(false, "Unexpected data format {:?}", e.data());
                }
            }
        });
        socket.set_onmessage(Some(message_callback.as_ref().unchecked_ref()));

        let error_callback = Closure::<dyn FnMut(_)>::new({
            let weak_shared = Arc::downgrade(&shared);
            move |_| {
                // The error event for error callback doesn't give any information and
                // generates error on the browser console we just signal it to the
                // stream.
                if let Some(shared) = weak_shared.upgrade() {
                    let mut locked = shared.lock();
                    locked.error = true;
                    locked.wake();
                }
            }
        });
        socket.set_onerror(Some(error_callback.as_ref().unchecked_ref()));

        let close_callback = Closure::<dyn FnMut(_)>::new({
            let weak_shared = Arc::downgrade(&shared);
            move |_| {
                if let Some(shared) = weak_shared.upgrade() {
                    let mut locked = shared.lock();
                    locked.closed = true;
                    locked.wake();
                }
            }
        });
        socket.set_onclose(Some(close_callback.as_ref().unchecked_ref()));

        // Manage closures memory.
        let closures = SendWrapper::new((
            open_callback,
            message_callback,
            error_callback,
            close_callback,
        ));

        shared.lock().closures = Some(closures);

        Self { shared }
    }
}

impl AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut shared = self.shared.lock();
        if shared.error {
            Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "Socket error")))
        } else if shared.closed {
            Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
        } else if shared.data.is_empty() {
            shared.read_waker = Some(cx.waker().clone());
            Poll::Pending
        } else {
            let n = shared.data.len().min(buf.len());
            for k in buf.iter_mut().take(n) {
                *k = shared.data.pop_front().unwrap();
            }
            Poll::Ready(Ok(n))
        }
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut shared = self.shared.lock();
        if shared.error {
            Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "Socket error")))
        } else if shared.closed {
            Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
        } else if !shared.opened {
            shared.write_waker = Some(cx.waker().clone());
            Poll::Pending
        } else {
            match shared.socket.send_with_u8_array(buf) {
                Ok(()) => Poll::Ready(Ok(buf.len())),
                Err(err) => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("Write error: {err:?}"),
                ))),
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Pending
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let shared = self.shared.lock();
        if shared.opened {
            let _ = shared.socket.close();
        }
    }
}
