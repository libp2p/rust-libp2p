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

use bytes::BytesMut;
use crate::{error::Error, tls};
use futures::{future::{self, Either, Loop}, prelude::*, try_ready};
use libp2p_core::{
    Transport,
    either::EitherOutput,
    multiaddr::{Protocol, Multiaddr},
    transport::{ListenerEvent, TransportError}
};
use log::{debug, error, trace};
use tokio_rustls::{client, server};
use soketto::{base, connection::{Connection, Mode}, handshake::{self, Redirect, Response}};
use std::io;
use tokio_codec::{Framed, FramedParts};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_rustls::webpki;
use url::Url;

/// Max. number of payload bytes of a single frame.
const MAX_DATA_SIZE: u64 = 256 * 1024 * 1024;

/// A Websocket transport whose output type is a [`Stream`] and [`Sink`] of
/// frame payloads (text or binary) but does not implement [`AsyncRead`] or
/// [`AsyncWrite`]. See [`WsConfig`] if you require the latter.
#[derive(Debug, Clone)]
pub struct WsConfig<T> {
    transport: T,
    max_data_size: u64,
    tls_config: Option<tls::Config>,
    max_redirects: u8
}

impl<T> WsConfig<T> {
    /// Create a new websocket transport based on another transport.
    pub fn new(transport: T) -> Self {
        WsConfig {
            transport,
            max_data_size: MAX_DATA_SIZE,
            tls_config: None,
            max_redirects: 0
        }
    }

    /// Return the configured maximum number of redirects.
    pub fn max_redirects(&self) -> u8 {
        self.max_redirects
    }

    /// Set max. number of redirects to follow.
    pub fn set_max_redirects(&mut self, max: u8) -> &mut Self {
        self.max_redirects = max;
        self
    }

    /// Get the max. frame data size we support.
    pub fn max_data_size(&self) -> u64 {
        self.max_data_size
    }

    /// Set the max. frame data size we support.
    pub fn set_max_data_size(&mut self, size: u64) -> &mut Self {
        self.max_data_size = size;
        self
    }

    /// Set the TLS configuration if TLS support is desired.
    pub fn set_tls_config(&mut self, c: tls::Config) -> &mut Self {
        self.tls_config = Some(c);
        self
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
    type Output = BytesConnection<T::Output>;
    type Error = Error<T::Error>;
    type Listener = Box<dyn Stream<Item = ListenerEvent<Self::ListenerUpgrade>, Error = Self::Error> + Send>;
    type ListenerUpgrade = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;
    type Dial = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let mut inner_addr = addr.clone();

        let (tls_config, proto) = match inner_addr.pop() {
            Some(p@Protocol::Wss(_)) =>
                if let Some(c) = self.tls_config {
                    (Some(c.clone()), p)
                } else {
                    debug!("/wss address but TLS is not configured");
                    return Err(TransportError::MultiaddrNotSupported(addr))
                }
            Some(p@Protocol::Ws(_)) => (None, p),
            _ => {
                trace!("{} is not a websocket multiaddr", addr);
                return Err(TransportError::MultiaddrNotSupported(addr))
            }
        };

        let max_size = self.max_data_size;
        let listen = self.transport.listen_on(inner_addr)
            .map_err(|e| e.map(Error::Transport))?
            .map_err(Error::Transport)
            .map(move |event| match event {
                ListenerEvent::NewAddress(mut a) => {
                    a = a.with(proto.clone());
                    debug!("Listening on {}", a);
                    ListenerEvent::NewAddress(a)
                }
                ListenerEvent::AddressExpired(mut a) => {
                    a = a.with(proto.clone());
                    ListenerEvent::AddressExpired(a)
                }
                ListenerEvent::Upgrade { upgrade, mut listen_addr, mut remote_addr } => {
                    listen_addr = listen_addr.with(proto.clone());
                    remote_addr = remote_addr.with(proto.clone());
                    let tls_config = tls_config.clone();
                    let upgraded = upgrade.map_err(Error::Transport)
                        .and_then(move |stream| {
                            trace!("incoming connection");
                            if let Some(tc) = tls_config { // begin TLS session
                                trace!("awaiting TLS handshake");
                                let future = tc.server.accept(stream)
                                    .map_err(|e| {
                                        debug!("TLS handshake failed: {}", e);
                                        Error::Tls(tls::Error::from(e))
                                    })
                                    .map(|s| EitherOutput::First(EitherOutput::Second(s)));
                                Either::A(future)
                            } else {
                                Either::B(future::ok(EitherOutput::Second(stream)))
                            }
                        })
                        .and_then(move |stream| {
                            trace!("receiving websocket handshake request");
                            Framed::new(stream, handshake::Server::new())
                                .into_future()
                                .map_err(|(e, _framed)| Error::Handshake(Box::new(e)))
                                .and_then(move |(request, framed)| {
                                    if let Some(r) = request {
                                        trace!("accepting websocket handshake request");
                                        let key = Vec::from(r.key());
                                        Either::A(framed.send(Ok(handshake::Accept::new(key)))
                                            .map_err(|e| Error::Base(Box::new(e)))
                                            .map(move |f| {
                                                trace!("websocket handshake successful");
                                                let c = new_connection(f, max_size, Mode::Server);
                                                BytesConnection { inner: c }
                                            }))
                                    } else {
                                        debug!("connection terminated during handshake");
                                        let e: io::Error = io::ErrorKind::ConnectionAborted.into();
                                        Either::B(future::err(Error::Handshake(Box::new(e))))
                                    }
                                })
                        });
                    ListenerEvent::Upgrade {
                        upgrade: Box::new(upgraded) as Box<dyn Future<Item = _, Error = _> + Send>,
                        listen_addr,
                        remote_addr
                    }
                }
            });
        Ok(Box::new(listen) as Box<_>)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // Quick sanity check of the provided Multiaddr.
        match addr.iter().last() {
            Some(Protocol::Ws(_)) => (),
            Some(Protocol::Wss(_)) =>
                if self.tls_config.is_none() {
                    debug!("/wss address but TLS is not configured");
                    return Err(TransportError::MultiaddrNotSupported(addr))
                }
            _ => {
                trace!("{} is not a websocket multiaddr", addr);
                return Err(TransportError::MultiaddrNotSupported(addr))
            }
        }
        let max_redirects = self.max_redirects;
        let future = future::loop_fn((addr, self, max_redirects), |(addr, cfg, remaining)| {
            dial(addr, cfg.clone()).and_then(move |result| match result {
                Either::A(redirect) => {
                    if remaining == 0 {
                        return Err(Error::TooManyRedirects)
                    }
                    let a = location_to_multiaddr(redirect.location())?;
                    Ok(Loop::Continue((a, cfg, remaining - 1)))
                }
                Either::B(conn) => Ok(Loop::Break(conn))
            })
        });
        Ok(Box::new(future) as Box<_>)
    }
}

fn dial<T>(mut address: Multiaddr, config: WsConfig<T>)
    -> impl Future<Item = Either<Redirect, BytesConnection<T::Output>>, Error = Error<T::Error>>
where
    T: Transport,
    T::Output: AsyncRead + AsyncWrite
{
    let WsConfig { transport, max_data_size, tls_config, .. } = config;

    let (host_port, dns_name) = match host_and_dnsname(&address) {
        Ok(x) => x,
        Err(e) => return Either::A(future::err(e))
    };

    let path = match address.pop() {
        Some(Protocol::Ws(p)) => p,
        Some(Protocol::Wss(p)) => p,
        Some(other) => {
            address.push(other);
            return Either::A(future::err(Error::InvalidMultiaddr(address)))
        }
        None => return Either::A(future::err(Error::InvalidMultiaddr(address)))
    };

    let dial = match transport.dial(address) {
        Ok(dial) => dial,
        Err(TransportError::MultiaddrNotSupported(a)) =>
            return Either::A(future::err(Error::InvalidMultiaddr(a))),
        Err(TransportError::Other(e)) =>
            return Either::A(future::err(Error::Transport(e)))
    };

    let future = dial.map_err(Error::Transport)
        .and_then(move |stream| {
            trace!("connected");
            if let Some(tc) = tls_config { // begin TLS session
                if let Some(dns_name) = dns_name {
                    trace!("starting TLS handshake");
                    let future = tc.client.connect(dns_name.as_ref(), stream)
                        .map_err(|e| {
                            debug!("TLS handshake failed: {}", e);
                            Error::Tls(tls::Error::from(e))
                        })
                        .map(|s| EitherOutput::First(EitherOutput::First(s)));
                    Either::A(future)
                } else {
                    Either::B(future::err(Error::Tls(tls::Error::InvalidDnsName(String::new()))))
                }
            } else { // continue with plain stream
                Either::B(future::ok(EitherOutput::Second(stream)))
            }
        })
        .and_then(move |stream| {
            trace!("sending websocket handshake request");
            let client = handshake::Client::new(host_port, path);
            Framed::new(stream, client)
                .send(())
                .map_err(|e| Error::Handshake(Box::new(e)))
                .and_then(|framed| {
                    trace!("awaiting websocket handshake response");
                    framed.into_future().map_err(|(e, _)| Error::Base(Box::new(e)))
                })
                .and_then(move |(response, framed)| {
                    match response {
                        None => {
                            debug!("connection terminated during handshake");
                            let e: io::Error = io::ErrorKind::ConnectionAborted.into();
                            return Err(Error::Handshake(Box::new(e)))
                        }
                        Some(Response::Redirect(r)) => {
                            error!("received redirect: {} ({})", r.location(), r.status_code());
                            return Ok(Either::A(r))
                        }
                        Some(Response::Accepted(_)) => {
                            trace!("websocket handshake successful")
                        }
                    }
                    let c = new_connection(framed, max_data_size, Mode::Client);
                    Ok(Either::B(BytesConnection { inner: c }))
                })
        });

    Either::B(future)
}

fn host_and_dnsname<T>(addr: &Multiaddr) -> Result<(String, Option<webpki::DNSName>), Error<T>> {
    let mut iter = addr.iter();
    match (iter.next(), iter.next()) {
        (Some(Protocol::Ip4(ip)), Some(Protocol::Tcp(port))) =>
            Ok((format!("{}:{}", ip, port), None)),
        (Some(Protocol::Ip6(ip)), Some(Protocol::Tcp(port))) =>
            Ok((format!("{}:{}", ip, port), None)),
        (Some(Protocol::Dns4(h)), Some(Protocol::Tcp(port))) =>
            Ok((format!("{}:{}", &h, port), Some(tls::dns_name_ref(&h)?.to_owned()))),
        (Some(Protocol::Dns6(h)), Some(Protocol::Tcp(port))) =>
            Ok((format!("{}:{}", &h, port), Some(tls::dns_name_ref(&h)?.to_owned()))),
        _ => {
            debug!("multi-address format not supported: {}", addr);
            Err(Error::InvalidMultiaddr(addr.clone()))
        }
    }
}

fn location_to_multiaddr<T>(location: &str) -> Result<Multiaddr, Error<T>> {
    match Url::parse(location) {
        Ok(url) => {
            let mut a = Multiaddr::empty();
            match url.host() {
                Some(url::Host::Domain(h)) => {
                    a.push(Protocol::Dns4(h.into()))
                }
                Some(url::Host::Ipv4(ip)) => {
                    a.push(Protocol::Ip4(ip))
                }
                Some(url::Host::Ipv6(ip)) => {
                    a.push(Protocol::Ip6(ip))
                }
                None => return Err(Error::InvalidRedirectLocation)
            }
            if let Some(p) = url.port() {
                a.push(Protocol::Tcp(p))
            }
            match url.scheme() {
                "https" | "wss" => {
                    a.push(Protocol::Wss(url.path().into()))
                }
                "http" | "ws" => {
                    a.push(Protocol::Ws(url.path().into()))
                }
                other => {
                    debug!("unsupported scheme: {}", other);
                    return Err(Error::InvalidRedirectLocation)
                }
            }
            Ok(a)
        }
        Err(e) => {
            debug!("failed to parse url as multi-address: {:?}", e);
            Err(Error::InvalidRedirectLocation)
        }
    }
}

/// Create a `Connection` from an existing `Framed` value.
fn new_connection<T, C>(framed: Framed<T, C>, max_size: u64, mode: Mode) -> Connection<T>
where
    T: AsyncRead + AsyncWrite
{
    let mut codec = base::Codec::new();
    codec.set_max_data_size(max_size);
    let old = framed.into_parts();
    let mut new = FramedParts::new(old.io, codec);
    new.read_buf = old.read_buf;
    new.write_buf = old.write_buf;
    let framed = Framed::from_parts(new);
    Connection::from_framed(framed, mode)
}

// BytesConnection ////////////////////////////////////////////////////////////////////////////////

/// A [`Stream`] and [`Sink`] that produces and consumes [`BytesMut`] values
/// which correspond to the payload data of websocket frames.
#[derive(Debug)]
pub struct BytesConnection<T> {
    inner: Connection<EitherOutput<EitherOutput<client::TlsStream<T>, server::TlsStream<T>>, T>>
}

impl<T: AsyncRead + AsyncWrite> Stream for BytesConnection<T> {
    type Item = BytesMut;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let data = try_ready!(self.inner.poll().map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
        Ok(Async::Ready(data.map(base::Data::into_bytes)))
    }
}

impl<T: AsyncRead + AsyncWrite> Sink for BytesConnection<T> {
    type SinkItem = BytesMut;
    type SinkError = io::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        let result = self.inner.start_send(base::Data::Binary(item))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e));

        if let AsyncSink::NotReady(data) = result? {
            Ok(AsyncSink::NotReady(data.into_bytes()))
        } else {
            Ok(AsyncSink::Ready)
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.poll_complete().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.close().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

