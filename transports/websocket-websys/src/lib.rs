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

//! Libp2p websocket transports built on [web-sys](https://rustwasm.github.io/wasm-bindgen/web-sys/index.html).
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
    pin::Pin,
    sync::Arc,
    task::Poll,
    task::{Context, Waker},
};

/// A Websocket transport that can be used in a wasm environment.
///
/// ## Example
///
/// To create an authenticated transport instance with Noise protocol and Yamux:
///
/// ```
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
pub struct Transport {
    _private: (),
}

impl libp2p_core::Transport for Transport {
    type Output = Connection;
    type Error = Error;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn listen_on(
        &mut self,
        _: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
    }

    fn remove_listener(&mut self, _id: ListenerId) -> bool {
        false
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let url = extract_websocket_url(&addr)
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr))?;

        Ok(async move {
            let socket = match WebSocket::new(&url) {
                Ok(ws) => ws,
                Err(_) => return Err(Error::invalid_websocket_url(&url)),
            };

            Ok(Connection::new(socket))
        }
        .boxed())
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        Err(TransportError::MultiaddrNotSupported(addr))
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
fn extract_websocket_url(addr: &Multiaddr) -> Option<String> {
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
#[error("{msg}")]
pub struct Error {
    msg: String,
}

impl Error {
    fn invalid_websocket_url(url: &str) -> Self {
        Self {
            msg: format!("Invalid websocket url: {url}"),
        }
    }
}

/// A Websocket connection created by the [`Transport`].
pub struct Connection {
    shared: Arc<Mutex<Shared>>,
}

struct Shared {
    state: State,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
    socket: SendWrapper<WebSocket>,
    closures: Option<SendWrapper<Closures>>,
}

enum State {
    Connecting,
    Open { buffer: Vec<u8> },
    Closing,
    Closed,
    Error,
}

impl Shared {
    fn wake_read_write(&self) {
        self.wake_read();
        self.wake_write()
    }

    fn wake_write(&self) {
        if let Some(waker) = &self.write_waker {
            waker.wake_by_ref();
        }
    }

    fn wake_read(&self) {
        if let Some(waker) = &self.read_waker {
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
            state: State::Connecting,
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
                    locked.state = State::Open {
                        buffer: Vec::default(),
                    };
                    locked.wake_read_write();
                }
            }
        });
        socket.set_onopen(Some(open_callback.as_ref().unchecked_ref()));

        let message_callback = Closure::<dyn FnMut(_)>::new({
            let weak_shared = Arc::downgrade(&shared);
            move |e: MessageEvent| {
                let buf = match e.data().dyn_into::<js_sys::ArrayBuffer>() {
                    Ok(buf) => buf,
                    _ => {
                        debug_assert!(false, "Unexpected data format {:?}", e.data());
                        return;
                    }
                };

                let shared = match weak_shared.upgrade() {
                    Some(shared) => shared,
                    None => return,
                };

                let mut locked = shared.lock();
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();

                if let State::Open { buffer } = &mut locked.state {
                    buffer.extend(bytes.into_iter());
                    locked.wake_read();
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
                    locked.state = State::Error;
                    locked.wake_read_write();
                }
            }
        });
        socket.set_onerror(Some(error_callback.as_ref().unchecked_ref()));

        let close_callback = Closure::<dyn FnMut(_)>::new({
            let weak_shared = Arc::downgrade(&shared);
            move |_| {
                if let Some(shared) = weak_shared.upgrade() {
                    let mut locked = shared.lock();
                    locked.state = State::Closed;
                    locked.wake_write();
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

        let buffer = match &mut shared.state {
            State::Connecting => {
                shared.read_waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
            State::Open { buffer } if buffer.is_empty() => {
                shared.read_waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
            State::Open { buffer } => buffer,
            State::Closed | State::Closing => {
                return Poll::Ready(Ok(0));
            }
            State::Error => {
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "Socket error")));
            }
        };

        let n = buffer.len().min(buf.len());

        let remaining_buffer = buffer.split_off(n);
        buf.copy_from_slice(buffer);
        buffer.clear();
        *buffer = remaining_buffer;

        Poll::Ready(Ok(n))
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut shared = self.shared.lock();

        match &shared.state {
            State::Connecting => {
                shared.write_waker = Some(cx.waker().clone());
                return Poll::Pending;
            }
            State::Closed | State::Closing => {
                return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
            }
            State::Error => {
                return Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "Socket error")));
            }
            State::Open { .. } => {}
        }

        shared.socket.send_with_u8_array(buf).map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "Failed to write data: {}",
                    e.as_string().unwrap_or_default()
                ),
            )
        })?;

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        let mut shared = self.shared.lock();

        match &shared.state {
            State::Open { .. } | State::Connecting => {
                let _ = shared.socket.close();
                shared.state = State::Closing;

                shared.write_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            State::Closing => {
                shared.write_waker = Some(cx.waker().clone());
                Poll::Pending
            }
            State::Closed => Poll::Ready(Ok(())),
            State::Error => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, "Socket error"))),
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        const GO_AWAY_STATUS_CODE: u16 = 1001; // See https://www.rfc-editor.org/rfc/rfc6455.html#section-7.4.1.

        let shared = self.shared.lock();

        if let State::Connecting | State::Open { .. } = shared.state {
            let _ = shared
                .socket
                .close_with_code_and_reason(GO_AWAY_STATUS_CODE, "connection dropped");
        }
    }
}
