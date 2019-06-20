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

pub mod error;
pub mod framed;
pub mod tls;

use error::Error;
use framed::BytesConnection;
use futures::prelude::*;
use libp2p_core::{
    ConnectedPoint,
    Transport,
    multiaddr::Multiaddr,
    transport::{map::{MapFuture, MapStream}, ListenerEvent, TransportError}
};
use rw_stream_sink::RwStreamSink;
use tokio_io::{AsyncRead, AsyncWrite};

/// A Websocket transport.
#[derive(Debug, Clone)]
pub struct WsConfig<T> {
    transport: framed::WsConfig<T>
}

impl<T> WsConfig<T> {
    /// Create a new websocket transport based on the given transport.
    pub fn new(transport: T) -> Self {
        framed::WsConfig::new(transport).into()
    }

    /// Return the configured maximum number of redirects.
    pub fn max_redirects(&self) -> u8 {
        self.transport.max_redirects()
    }

    /// Set max. number of redirects to follow.
    pub fn set_max_redirects(&mut self, max: u8) -> &mut Self {
        self.transport.set_max_redirects(max);
        self
    }

    /// Get the max. frame data size we support.
    pub fn max_data_size(&self) -> u64 {
        self.transport.max_data_size()
    }

    /// Set the max. frame data size we support.
    pub fn set_max_data_size(&mut self, size: u64) -> &mut Self {
        self.transport.set_max_data_size(size);
        self
    }

    /// Set the TLS configuration if TLS support is desired.
    pub fn set_tls_config(&mut self, c: tls::Config) -> &mut Self {
        self.transport.set_tls_config(c);
        self
    }

    /// Should the deflate extension (RFC 7692) be used if supported?
    pub fn use_deflate(&mut self, flag: bool) -> &mut Self {
        self.transport.use_deflate(flag);
        self
    }
}

impl<T> From<framed::WsConfig<T>> for WsConfig<T> {
    fn from(framed: framed::WsConfig<T>) -> Self {
        WsConfig {
            transport: framed
        }
    }
}

impl<T> Transport for WsConfig<T>
where
    T: Transport + Send + Clone + 'static,
    T::Error: Send + 'static,
    T::Dial: Send + 'static,
    T::Listener: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Output: AsyncRead + AsyncWrite + Send + 'static
{
    type Output = RwStreamSink<BytesConnection<T::Output>>;
    type Error = Error<T::Error>;
    type Listener = MapStream<InnerStream<T::Output, T::Error>, WrapperFn<T::Output>>;
    type ListenerUpgrade = MapFuture<InnerFuture<T::Output, T::Error>, WrapperFn<T::Output>>;
    type Dial = MapFuture<InnerFuture<T::Output, T::Error>, WrapperFn<T::Output>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        self.transport.map(wrap_connection as WrapperFn<T::Output>).listen_on(addr)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.transport.map(wrap_connection as WrapperFn<T::Output>).dial(addr)
    }
}

/// Type alias corresponding to `framed::WsConfig::Listener`.
pub type InnerStream<T, E> =
    Box<(dyn Stream<Error = Error<E>, Item = ListenerEvent<InnerFuture<T, E>>> + Send)>;

/// Type alias corresponding to `framed::WsConfig::Dial` and `framed::WsConfig::ListenerUpgrade`.
pub type InnerFuture<T, E> =
    Box<(dyn Future<Item = BytesConnection<T>, Error = Error<E>> + Send)>;

/// Function type that wraps a websocket connection (see. `wrap_connection`).
pub type WrapperFn<T> =
    fn(BytesConnection<T>, ConnectedPoint) -> RwStreamSink<BytesConnection<T>>;

/// Wrap a websocket connection producing data frames into a `RwStreamSink`
/// implementing `AsyncRead` + `AsyncWrite`.
fn wrap_connection<T>(c: BytesConnection<T>, _: ConnectedPoint) -> RwStreamSink<BytesConnection<T>>
where
    T: AsyncRead + AsyncWrite
{
    RwStreamSink::new(c)
}

// Tests //////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use libp2p_tcp as tcp;
    use tokio::runtime::current_thread::Runtime;
    use futures::{Future, Stream};
    use libp2p_core::{
        Transport,
        multiaddr::Protocol,
        transport::ListenerEvent
    };
    use super::WsConfig;

    #[test]
    fn dialer_connects_to_listener_ipv4() {
        let ws_config = WsConfig::new(tcp::TcpConfig::new());

        let mut listener = ws_config.clone()
            .listen_on("/ip4/127.0.0.1/tcp/0/ws".parse().unwrap())
            .unwrap();

        let addr = listener.by_ref().wait()
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert_eq!(Some(Protocol::Ws("/".into())), addr.iter().nth(2));
        assert_ne!(Some(Protocol::Tcp(0)), addr.iter().nth(1));

        let listener = listener
            .filter_map(ListenerEvent::into_upgrade)
            .into_future()
            .map_err(|(e, _)| e)
            .and_then(|(c, _)| c.unwrap().0);

        let dialer = ws_config.clone().dial(addr.clone()).unwrap();

        let future = listener
            .select(dialer)
            .map_err(|(e, _)| e)
            .and_then(|(_, n)| n);
        let mut rt = Runtime::new().unwrap();
        let _ = rt.block_on(future).unwrap();
    }

    #[test]
    fn dialer_connects_to_listener_ipv6() {
        let ws_config = WsConfig::new(tcp::TcpConfig::new());

        let mut listener = ws_config.clone()
            .listen_on("/ip6/::1/tcp/0/ws".parse().unwrap())
            .unwrap();

        let addr = listener.by_ref().wait()
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert_eq!(Some(Protocol::Ws("/".into())), addr.iter().nth(2));
        assert_ne!(Some(Protocol::Tcp(0)), addr.iter().nth(1));

        let listener = listener
            .filter_map(ListenerEvent::into_upgrade)
            .into_future()
            .map_err(|(e, _)| e)
            .and_then(|(c, _)| c.unwrap().0);

        let dialer = ws_config.clone().dial(addr.clone()).unwrap();

        let future = listener
            .select(dialer)
            .map_err(|(e, _)| e)
            .and_then(|(_, n)| n);

        let mut rt = Runtime::new().unwrap();
        let _ = rt.block_on(future).unwrap();
    }
}
