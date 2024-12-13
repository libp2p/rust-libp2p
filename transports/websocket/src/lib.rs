// Copyright 2017-2019 Parity Technologies (UK) Ltd.
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

//! Implementation of the libp2p `Transport` trait for Websockets.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

pub mod error;
pub mod framed;
mod quicksink;
pub mod tls;

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use error::Error;
use framed::{Connection, Incoming};
use futures::{future::BoxFuture, prelude::*, ready};
use libp2p_core::{
    connection::ConnectedPoint,
    multiaddr::Multiaddr,
    transport::{map::MapFuture, DialOpts, ListenerId, TransportError, TransportEvent},
    Transport,
};
use rw_stream_sink::RwStreamSink;

/// A Websocket transport.
///
/// DO NOT wrap this transport with a DNS transport if you want Secure Websockets to work.
///
/// A Secure Websocket transport needs to wrap DNS transport to resolve domain names after
/// they are checked against the remote certificates. Use a combination of DNS and TCP transports
/// to build a Secure Websocket transport.
///
/// If you don't need Secure Websocket's support, use a plain TCP transport as an inner transport.
///
/// # Dependencies
///
/// This transport requires the `zlib` shared library to be installed on the system.
///
/// Future releases might lift this requirement, see <https://github.com/paritytech/soketto/issues/72>.
///
/// # Examples
///
/// Secure Websocket transport:
///
/// ```
/// # use futures::future;
/// # use libp2p_core::{transport::ListenerId, Transport};
/// # use libp2p_dns as dns;
/// # use libp2p_tcp as tcp;
/// # use libp2p_websocket as websocket;
/// # use rcgen::generate_simple_self_signed;
/// # use std::pin::Pin;
/// #
/// # #[async_std::main]
/// # async fn main() {
///
/// let mut transport = websocket::WsConfig::new(
///     dns::async_std::Transport::system(tcp::async_io::Transport::new(tcp::Config::default()))
///         .await
///         .unwrap(),
/// );
///
/// let rcgen_cert = generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
/// let priv_key = websocket::tls::PrivateKey::new(rcgen_cert.serialize_private_key_der());
/// let cert = websocket::tls::Certificate::new(rcgen_cert.serialize_der().unwrap());
/// transport.set_tls_config(websocket::tls::Config::new(priv_key, vec![cert]).unwrap());
///
/// let id = transport
///     .listen_on(
///         ListenerId::next(),
///         "/ip4/127.0.0.1/tcp/0/tls/ws".parse().unwrap(),
///     )
///     .unwrap();
///
/// let addr = future::poll_fn(|cx| Pin::new(&mut transport).poll(cx))
///     .await
///     .into_new_address()
///     .unwrap();
/// println!("Listening on {addr}");
///
/// # }
/// ```
///
/// Plain Websocket transport:
///
/// ```
/// # use futures::future;
/// # use libp2p_core::{transport::ListenerId, Transport};
/// # use libp2p_dns as dns;
/// # use libp2p_tcp as tcp;
/// # use libp2p_websocket as websocket;
/// # use std::pin::Pin;
/// #
/// # #[async_std::main]
/// # async fn main() {
///
/// let mut transport =
///     websocket::WsConfig::new(tcp::async_io::Transport::new(tcp::Config::default()));
///
/// let id = transport
///     .listen_on(
///         ListenerId::next(),
///         "/ip4/127.0.0.1/tcp/0/ws".parse().unwrap(),
///     )
///     .unwrap();
///
/// let addr = future::poll_fn(|cx| Pin::new(&mut transport).poll(cx))
///     .await
///     .into_new_address()
///     .unwrap();
/// println!("Listening on {addr}");
///
/// # }
/// ```
#[derive(Debug)]
pub struct WsConfig<T: Transport>
where
    T: Transport,
    T::Output: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    transport: libp2p_core::transport::map::Map<framed::WsConfig<T>, WrapperFn<T::Output>>,
}

impl<T: Transport> WsConfig<T>
where
    T: Transport + Send + Unpin + 'static,
    T::Error: Send + 'static,
    T::Dial: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Output: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    /// Create a new websocket transport based on the given transport.
    ///
    /// > **Note*: The given transport must be based on TCP/IP and should
    /// > usually incorporate DNS resolution, though the latter is not
    /// > strictly necessary if one wishes to only use the `Ws` protocol
    /// > with known IP addresses and ports. See [`libp2p-tcp`](https://docs.rs/libp2p-tcp/)
    /// > and [`libp2p-dns`](https://docs.rs/libp2p-dns) for constructing
    /// > the inner transport.
    pub fn new(transport: T) -> Self {
        Self {
            transport: framed::WsConfig::new(transport)
                .map(wrap_connection as WrapperFn<T::Output>),
        }
    }

    /// Return the configured maximum number of redirects.
    pub fn max_redirects(&self) -> u8 {
        self.transport.inner().max_redirects()
    }

    /// Set max. number of redirects to follow.
    pub fn set_max_redirects(&mut self, max: u8) -> &mut Self {
        self.transport.inner_mut().set_max_redirects(max);
        self
    }

    /// Get the max. frame data size we support.
    pub fn max_data_size(&self) -> usize {
        self.transport.inner().max_data_size()
    }

    /// Set the max. frame data size we support.
    pub fn set_max_data_size(&mut self, size: usize) -> &mut Self {
        self.transport.inner_mut().set_max_data_size(size);
        self
    }

    /// Set the TLS configuration if TLS support is desired.
    pub fn set_tls_config(&mut self, c: tls::Config) -> &mut Self {
        self.transport.inner_mut().set_tls_config(c);
        self
    }
}

impl<T> Transport for WsConfig<T>
where
    T: Transport + Send + Unpin + 'static,
    T::Error: Send + 'static,
    T::Dial: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Output: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = RwStreamSink<BytesConnection<T::Output>>;
    type Error = Error<T::Error>;
    type ListenerUpgrade = MapFuture<InnerFuture<T::Output, T::Error>, WrapperFn<T::Output>>;
    type Dial = MapFuture<InnerFuture<T::Output, T::Error>, WrapperFn<T::Output>>;

    fn listen_on(
        &mut self,
        id: ListenerId,
        addr: Multiaddr,
    ) -> Result<(), TransportError<Self::Error>> {
        self.transport.listen_on(id, addr)
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.transport.remove_listener(id)
    }

    fn dial(
        &mut self,
        addr: Multiaddr,
        opts: DialOpts,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.transport.dial(addr, opts)
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        Pin::new(&mut self.transport).poll(cx)
    }
}

/// Type alias corresponding to `framed::WsConfig::Dial` and `framed::WsConfig::ListenerUpgrade`.
pub type InnerFuture<T, E> = BoxFuture<'static, Result<Connection<T>, Error<E>>>;

/// Function type that wraps a websocket connection (see. `wrap_connection`).
pub type WrapperFn<T> = fn(Connection<T>, ConnectedPoint) -> RwStreamSink<BytesConnection<T>>;

/// Wrap a websocket connection producing data frames into a `RwStreamSink`
/// implementing `AsyncRead` + `AsyncWrite`.
fn wrap_connection<T>(c: Connection<T>, _: ConnectedPoint) -> RwStreamSink<BytesConnection<T>>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    RwStreamSink::new(BytesConnection(c))
}

/// The websocket connection.
#[derive(Debug)]
pub struct BytesConnection<T>(Connection<T>);

impl<T> Stream for BytesConnection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Item = io::Result<Vec<u8>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(item) = ready!(self.0.try_poll_next_unpin(cx)?) {
                if let Incoming::Data(payload) = item {
                    return Poll::Ready(Some(Ok(payload.into_bytes())));
                }
            } else {
                return Poll::Ready(None);
            }
        }
    }
}

impl<T> Sink<Vec<u8>> for BytesConnection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: Vec<u8>) -> io::Result<()> {
        Pin::new(&mut self.0).start_send(framed::OutgoingData::Binary(item))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_close(cx)
    }
}

// Tests //////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use futures::prelude::*;
    use libp2p_core::{
        multiaddr::Protocol,
        transport::{DialOpts, ListenerId, PortUse},
        Endpoint, Multiaddr, Transport,
    };
    use libp2p_identity::PeerId;
    use libp2p_tcp as tcp;

    use super::WsConfig;

    #[test]
    fn dialer_connects_to_listener_ipv4() {
        let a = "/ip4/127.0.0.1/tcp/0/ws".parse().unwrap();
        futures::executor::block_on(connect(a))
    }

    #[test]
    fn dialer_connects_to_listener_ipv6() {
        let a = "/ip6/::1/tcp/0/ws".parse().unwrap();
        futures::executor::block_on(connect(a))
    }

    fn new_ws_config() -> WsConfig<tcp::async_io::Transport> {
        WsConfig::new(tcp::async_io::Transport::new(tcp::Config::default()))
    }

    async fn connect(listen_addr: Multiaddr) {
        let mut ws_config = new_ws_config().boxed();
        ws_config
            .listen_on(ListenerId::next(), listen_addr)
            .expect("listener");

        let addr = ws_config
            .next()
            .await
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert_eq!(Some(Protocol::Ws("/".into())), addr.iter().nth(2));
        assert_ne!(Some(Protocol::Tcp(0)), addr.iter().nth(1));

        let inbound = async move {
            let (conn, _addr) = ws_config
                .select_next_some()
                .map(|ev| ev.into_incoming())
                .await
                .unwrap();
            conn.await
        };

        let outbound = new_ws_config()
            .boxed()
            .dial(
                addr.with(Protocol::P2p(PeerId::random())),
                DialOpts {
                    role: Endpoint::Dialer,
                    port_use: PortUse::New,
                },
            )
            .unwrap();

        let (a, b) = futures::join!(inbound, outbound);
        a.and(b).unwrap();
    }
}
