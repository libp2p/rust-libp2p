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

#![allow(unexpected_cfgs)]

mod web_context;

use std::{
    cmp::min,
    pin::Pin,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    task::{Context, Poll},
};

use bytes::BytesMut;
use futures::{future::Ready, io, prelude::*, task::AtomicWaker};
use js_sys::Array;
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{DialOpts, ListenerId, TransportError, TransportEvent},
};
use send_wrapper::SendWrapper;
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

use crate::web_context::WebContext;

/// A Websocket transport that can be used in a wasm environment.
///
/// ## Example
///
/// To create an authenticated transport instance with Noise protocol and Yamux:
///
/// ```
/// # use libp2p_core::{upgrade::Version, Transport};
/// # use libp2p_identity::Keypair;
/// # use libp2p_yamux as yamux;
/// # use libp2p_noise as noise;
/// let local_key = Keypair::generate_ed25519();
/// let transport = libp2p_websocket_websys::Transport::default()
///     .upgrade(Version::V1)
///     .authenticate(noise::Config::new(&local_key).unwrap())
///     .multiplex(yamux::Config::default())
///     .boxed();
/// ```
#[derive(Default)]
pub struct Transport {
    _private: (),
}

/// Arbitrary, maximum amount we are willing to buffer before we throttle our user.
const MAX_BUFFER: usize = 1024 * 1024;

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

    fn dial(
        &mut self,
        addr: Multiaddr,
        dial_opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        if dial_opts.role.is_listener() {
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let url =
            extract_websocket_url(&addr).ok_or(TransportError::MultiaddrNotSupported(addr))?;

        Ok(async move {
            let socket = match WebSocket::new(&url) {
                Ok(ws) => ws,
                Err(_) => return Err(Error::invalid_websocket_url(&url)),
            };

            Ok(Connection::new(socket))
        }
        .boxed())
    }

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> std::task::Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Poll::Pending
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
        | (Some(Protocol::Dns6(h)), Some(Protocol::Tcp(port))) => {
            format!("{}:{}", &h, port)
        }
        _ => return None,
    };

    let (scheme, wspath) = match (protocols.next(), protocols.next()) {
        (Some(Protocol::Tls), Some(Protocol::Ws(path))) => ("wss", path.into_owned()),
        (Some(Protocol::Ws(path)), _) => ("ws", path.into_owned()),
        (Some(Protocol::Wss(path)), _) => ("wss", path.into_owned()),
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
    inner: SendWrapper<Inner>,
}

struct Inner {
    socket: WebSocket,

    new_data_waker: Rc<AtomicWaker>,
    read_buffer: Rc<Mutex<BytesMut>>,

    /// Waker for when we are waiting for the WebSocket to be opened.
    open_waker: Rc<AtomicWaker>,

    /// Waker for when we are waiting to write (again) to the WebSocket because we previously
    /// exceeded the [`MAX_BUFFER`] threshold.
    write_waker: Rc<AtomicWaker>,

    /// Waker for when we are waiting for the WebSocket to be closed.
    close_waker: Rc<AtomicWaker>,

    /// Whether the connection errored.
    errored: Rc<AtomicBool>,

    // Store the closures for proper garbage collection.
    // These are wrapped in an [`Rc`] so we can implement [`Clone`].
    _on_open_closure: Rc<Closure<dyn FnMut(Event)>>,
    _on_buffered_amount_low_closure: Rc<Closure<dyn FnMut(Event)>>,
    _on_close_closure: Rc<Closure<dyn FnMut(CloseEvent)>>,
    _on_error_closure: Rc<Closure<dyn FnMut(CloseEvent)>>,
    _on_message_closure: Rc<Closure<dyn FnMut(MessageEvent)>>,
    buffered_amount_low_interval: i32,
}

impl Inner {
    fn ready_state(&self) -> ReadyState {
        match self.socket.ready_state() {
            0 => ReadyState::Connecting,
            1 => ReadyState::Open,
            2 => ReadyState::Closing,
            3 => ReadyState::Closed,
            unknown => unreachable!("invalid `ReadyState` value: {unknown}"),
        }
    }

    fn poll_open(&mut self, cx: &Context<'_>) -> Poll<io::Result<()>> {
        match self.ready_state() {
            ReadyState::Connecting => {
                self.open_waker.register(cx.waker());
                Poll::Pending
            }
            ReadyState::Open => Poll::Ready(Ok(())),
            ReadyState::Closed | ReadyState::Closing => {
                Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
            }
        }
    }

    fn error_barrier(&self) -> io::Result<()> {
        if self.errored.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }

        Ok(())
    }
}

/// The state of the WebSocket.
///
/// See <https://developer.mozilla.org/en-US/docs/Web/API/WebSocket/readyState>.
#[derive(PartialEq)]
enum ReadyState {
    Connecting,
    Open,
    Closing,
    Closed,
}

impl Connection {
    fn new(socket: WebSocket) -> Self {
        socket.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let open_waker = Rc::new(AtomicWaker::new());
        let onopen_closure = Closure::<dyn FnMut(_)>::new({
            let open_waker = open_waker.clone();
            move |_| {
                open_waker.wake();
            }
        });
        socket.set_onopen(Some(onopen_closure.as_ref().unchecked_ref()));

        let close_waker = Rc::new(AtomicWaker::new());
        let onclose_closure = Closure::<dyn FnMut(_)>::new({
            let close_waker = close_waker.clone();
            move |_| {
                close_waker.wake();
            }
        });
        socket.set_onclose(Some(onclose_closure.as_ref().unchecked_ref()));

        let errored = Rc::new(AtomicBool::new(false));
        let onerror_closure = Closure::<dyn FnMut(_)>::new({
            let errored = errored.clone();
            move |_| {
                errored.store(true, Ordering::SeqCst);
            }
        });
        socket.set_onerror(Some(onerror_closure.as_ref().unchecked_ref()));

        let read_buffer = Rc::new(Mutex::new(BytesMut::new()));
        let new_data_waker = Rc::new(AtomicWaker::new());
        let onmessage_closure = Closure::<dyn FnMut(_)>::new({
            let read_buffer = read_buffer.clone();
            let new_data_waker = new_data_waker.clone();
            let errored = errored.clone();
            move |e: MessageEvent| {
                let data = js_sys::Uint8Array::new(&e.data());

                let mut read_buffer = read_buffer.lock().unwrap();

                if read_buffer.len() + data.length() as usize > MAX_BUFFER {
                    tracing::warn!("Remote is overloading us with messages, closing connection");
                    errored.store(true, Ordering::SeqCst);

                    return;
                }

                read_buffer.extend_from_slice(&data.to_vec());
                new_data_waker.wake();
            }
        });
        socket.set_onmessage(Some(onmessage_closure.as_ref().unchecked_ref()));

        let write_waker = Rc::new(AtomicWaker::new());
        let on_buffered_amount_low_closure = Closure::<dyn FnMut(_)>::new({
            let write_waker = write_waker.clone();
            let socket = socket.clone();
            move |_| {
                if socket.buffered_amount() == 0 {
                    write_waker.wake();
                }
            }
        });
        let buffered_amount_low_interval = WebContext::new()
            .expect("to have a window or worker context")
            .set_interval_with_callback_and_timeout_and_arguments(
                on_buffered_amount_low_closure.as_ref().unchecked_ref(),
                // Chosen arbitrarily and likely worth tuning. Due to low impact of the /ws
                // transport, no further effort was invested at the time.
                100,
                &Array::new(),
            )
            .expect("to be able to set an interval");

        Self {
            inner: SendWrapper::new(Inner {
                socket,
                new_data_waker,
                read_buffer,
                open_waker,
                write_waker,
                close_waker,
                errored,
                _on_open_closure: Rc::new(onopen_closure),
                _on_buffered_amount_low_closure: Rc::new(on_buffered_amount_low_closure),
                _on_close_closure: Rc::new(onclose_closure),
                _on_error_closure: Rc::new(onerror_closure),
                _on_message_closure: Rc::new(onmessage_closure),
                buffered_amount_low_interval,
            }),
        }
    }

    fn buffered_amount(&self) -> usize {
        self.inner.socket.buffered_amount() as usize
    }
}

impl AsyncRead for Connection {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();
        this.inner.error_barrier()?;
        futures::ready!(this.inner.poll_open(cx))?;

        let mut read_buffer = this.inner.read_buffer.lock().unwrap();

        if read_buffer.is_empty() {
            this.inner.new_data_waker.register(cx.waker());
            return Poll::Pending;
        }

        // Ensure that we:
        // - at most return what the caller can read (`buf.len()`)
        // - at most what we have (`read_buffer.len()`)
        let split_index = min(buf.len(), read_buffer.len());

        let bytes_to_return = read_buffer.split_to(split_index);
        let len = bytes_to_return.len();
        buf[..len].copy_from_slice(&bytes_to_return);

        Poll::Ready(Ok(len))
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        this.inner.error_barrier()?;
        futures::ready!(this.inner.poll_open(cx))?;

        debug_assert!(this.buffered_amount() <= MAX_BUFFER);
        let remaining_space = MAX_BUFFER - this.buffered_amount();

        if remaining_space == 0 {
            this.inner.write_waker.register(cx.waker());
            return Poll::Pending;
        }

        let bytes_to_send = min(buf.len(), remaining_space);

        if this
            .inner
            .socket
            .send_with_u8_array(&buf[..bytes_to_send])
            .is_err()
        {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }

        Poll::Ready(Ok(bytes_to_send))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.buffered_amount() == 0 {
            return Poll::Ready(Ok(()));
        }

        self.inner.error_barrier()?;

        self.inner.write_waker.register(cx.waker());
        Poll::Pending
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        const REGULAR_CLOSE: u16 = 1000; // See https://www.rfc-editor.org/rfc/rfc6455.html#section-7.4.1.

        if self.inner.ready_state() == ReadyState::Closed {
            return Poll::Ready(Ok(()));
        }

        self.inner.error_barrier()?;

        if self.inner.ready_state() != ReadyState::Closing {
            let _ = self
                .inner
                .socket
                .close_with_code_and_reason(REGULAR_CLOSE, "user initiated");
        }

        self.inner.close_waker.register(cx.waker());
        Poll::Pending
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // Unset event listeners, as otherwise they will be called by JS after the handlers have
        // already been dropped.
        self.inner.socket.set_onclose(None);
        self.inner.socket.set_onerror(None);
        self.inner.socket.set_onopen(None);
        self.inner.socket.set_onmessage(None);

        // In browsers, userland code is not allowed to use any other status code than 1000: https://websockets.spec.whatwg.org/#dom-websocket-close
        const REGULAR_CLOSE: u16 = 1000; // See https://www.rfc-editor.org/rfc/rfc6455.html#section-7.4.1.

        if let ReadyState::Connecting | ReadyState::Open = self.inner.ready_state() {
            let _ = self
                .inner
                .socket
                .close_with_code_and_reason(REGULAR_CLOSE, "connection dropped");
        }

        WebContext::new()
            .expect("to have a window or worker context")
            .clear_interval_with_handle(self.inner.buffered_amount_low_interval);
    }
}

#[cfg(test)]
mod tests {
    use libp2p_identity::PeerId;

    use super::*;

    #[test]
    fn extract_url() {
        let peer_id = PeerId::random();

        // Check `/tls/ws`
        let addr = "/dns4/example.com/tcp/2222/tls/ws"
            .parse::<Multiaddr>()
            .unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "wss://example.com:2222/");

        // Check `/tls/ws` with `/p2p`
        let addr = format!("/dns4/example.com/tcp/2222/tls/ws/p2p/{peer_id}")
            .parse()
            .unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "wss://example.com:2222/");

        // Check `/tls/ws` with `/ip4`
        let addr = "/ip4/127.0.0.1/tcp/2222/tls/ws"
            .parse::<Multiaddr>()
            .unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "wss://127.0.0.1:2222/");

        // Check `/tls/ws` with `/ip6`
        let addr = "/ip6/::1/tcp/2222/tls/ws".parse::<Multiaddr>().unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "wss://[::1]:2222/");

        // Check `/wss`
        let addr = "/dns4/example.com/tcp/2222/wss"
            .parse::<Multiaddr>()
            .unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "wss://example.com:2222/");

        // Check `/wss` with `/p2p`
        let addr = format!("/dns4/example.com/tcp/2222/wss/p2p/{peer_id}")
            .parse()
            .unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "wss://example.com:2222/");

        // Check `/wss` with `/ip4`
        let addr = "/ip4/127.0.0.1/tcp/2222/wss".parse::<Multiaddr>().unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "wss://127.0.0.1:2222/");

        // Check `/wss` with `/ip6`
        let addr = "/ip6/::1/tcp/2222/wss".parse::<Multiaddr>().unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "wss://[::1]:2222/");

        // Check `/ws`
        let addr = "/dns4/example.com/tcp/2222/ws"
            .parse::<Multiaddr>()
            .unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "ws://example.com:2222/");

        // Check `/ws` with `/p2p`
        let addr = format!("/dns4/example.com/tcp/2222/ws/p2p/{peer_id}")
            .parse()
            .unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "ws://example.com:2222/");

        // Check `/ws` with `/ip4`
        let addr = "/ip4/127.0.0.1/tcp/2222/ws".parse::<Multiaddr>().unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "ws://127.0.0.1:2222/");

        // Check `/ws` with `/ip6`
        let addr = "/ip6/::1/tcp/2222/ws".parse::<Multiaddr>().unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "ws://[::1]:2222/");

        // Check `/ws` with `/ip4`
        let addr = "/ip4/127.0.0.1/tcp/2222/ws".parse::<Multiaddr>().unwrap();
        let url = extract_websocket_url(&addr).unwrap();
        assert_eq!(url, "ws://127.0.0.1:2222/");

        // Check that `/tls/wss` is invalid
        let addr = "/ip4/127.0.0.1/tcp/2222/tls/wss"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(extract_websocket_url(&addr).is_none());

        // Check `/dnsaddr`
        let addr = "/dnsaddr/example.com/tcp/2222/ws"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(extract_websocket_url(&addr).is_none());

        // Check non-ws address
        let addr = "/ip4/127.0.0.1/tcp/2222".parse::<Multiaddr>().unwrap();
        assert!(extract_websocket_url(&addr).is_none());
    }
}
