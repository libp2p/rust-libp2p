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
use log::{debug, trace};
use tokio_rustls::{client, server};
use soketto::{
    base,
    connection::{Connection, Mode},
    extension::deflate::Deflate,
    handshake::{self, Redirect, Response}
};
use std::{convert::TryFrom, io};
use tokio_codec::{Framed, FramedParts};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_rustls::webpki;
use url::Url;

/// Max. number of payload bytes of a single frame.
const MAX_DATA_SIZE: u64 = 256 * 1024 * 1024;

/// A Websocket transport whose output type is a [`Stream`] and [`Sink`] of
/// frame payloads which does not implement [`AsyncRead`] or
/// [`AsyncWrite`]. See [`crate::WsConfig`] if you require the latter.
#[derive(Debug, Clone)]
pub struct WsConfig<T> {
    transport: T,
    max_data_size: u64,
    tls_config: tls::Config,
    max_redirects: u8,
    use_deflate: bool
}

impl<T> WsConfig<T> {
    /// Create a new websocket transport based on another transport.
    pub fn new(transport: T) -> Self {
        WsConfig {
            transport,
            max_data_size: MAX_DATA_SIZE,
            tls_config: tls::Config::client(),
            max_redirects: 0,
            use_deflate: false
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
        self.tls_config = c;
        self
    }

    /// Should the deflate extension (RFC 7692) be used if supported?
    pub fn use_deflate(&mut self, flag: bool) -> &mut Self {
        self.use_deflate = flag;
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

        let (use_tls, proto) = match inner_addr.pop() {
            Some(p@Protocol::Wss(_)) =>
                if self.tls_config.server.is_some() {
                    (true, p)
                } else {
                    debug!("/wss address but TLS server support is not configured");
                    return Err(TransportError::MultiaddrNotSupported(addr))
                }
            Some(p@Protocol::Ws(_)) => (false, p),
            _ => {
                debug!("{} is not a websocket multiaddr", addr);
                return Err(TransportError::MultiaddrNotSupported(addr))
            }
        };

        let tls_config = self.tls_config;
        let max_size = self.max_data_size;
        let use_deflate = self.use_deflate;
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
                ListenerEvent::Upgrade { upgrade, mut local_addr, mut remote_addr } => {
                    local_addr = local_addr.with(proto.clone());
                    remote_addr = remote_addr.with(proto.clone());
                    let remote1 = remote_addr.clone(); // used for logging
                    let remote2 = remote_addr.clone(); // used for logging
                    let tls_config = tls_config.clone();
                    let upgraded = upgrade.map_err(Error::Transport)
                        .and_then(move |stream| {
                            trace!("incoming connection from {}", remote1);
                            if use_tls { // begin TLS session
                                let server = tls_config.server.expect("for use_tls we checked server");
                                trace!("awaiting TLS handshake with {}", remote1);
                                let future = server.accept(stream)
                                    .map_err(move |e| {
                                        debug!("TLS handshake with {} failed: {}", remote1, e);
                                        Error::Tls(tls::Error::from(e))
                                    })
                                    .map(|s| EitherOutput::First(EitherOutput::Second(s)));
                                Either::A(future)
                            } else { // continue with plain stream
                                Either::B(future::ok(EitherOutput::Second(stream)))
                            }
                        })
                        .and_then(move |stream| {
                            trace!("receiving websocket handshake request from {}", remote2);
                            let mut s = handshake::Server::new();
                            if use_deflate {
                                s.add_extension(Box::new(Deflate::new(Mode::Server)));
                            }
                            Framed::new(stream, s)
                                .into_future()
                                .map_err(|(e, _framed)| Error::Handshake(Box::new(e)))
                                .and_then(move |(request, framed)| {
                                    if let Some(r) = request {
                                        trace!("accepting websocket handshake request from {}", remote2);
                                        let key = Vec::from(r.key());
                                        Either::A(framed.send(Ok(handshake::Accept::new(key)))
                                            .map_err(|e| Error::Base(Box::new(e)))
                                            .map(move |f| {
                                                trace!("websocket handshake with {} successful", remote2);
                                                let (mut handshake, mut c) =
                                                    new_connection(f, max_size, Mode::Server);
                                                c.add_extensions(handshake.drain_extensions());
                                                BytesConnection { inner: c }
                                            }))
                                    } else {
                                        debug!("connection to {} terminated during handshake", remote2);
                                        let e: io::Error = io::ErrorKind::ConnectionAborted.into();
                                        Either::B(future::err(Error::Handshake(Box::new(e))))
                                    }
                                })
                        });
                    ListenerEvent::Upgrade {
                        upgrade: Box::new(upgraded) as Box<dyn Future<Item = _, Error = _> + Send>,
                        local_addr,
                        remote_addr
                    }
                }
            });
        Ok(Box::new(listen) as Box<_>)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // Quick sanity check of the provided Multiaddr.
        if let Some(Protocol::Ws(_)) | Some(Protocol::Wss(_)) = addr.iter().last() {
            // ok
        } else {
            debug!("{} is not a websocket multiaddr", addr);
            return Err(TransportError::MultiaddrNotSupported(addr))
        }
        // We are looping here in order to follow redirects (if any):
        let max_redirects = self.max_redirects;
        let future = future::loop_fn((addr, self, max_redirects), |(addr, cfg, remaining)| {
            dial(addr, cfg.clone()).and_then(move |result| match result {
                Either::A(redirect) => {
                    if remaining == 0 {
                        debug!("too many redirects");
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

/// Attempty to dial the given address and perform a websocket handshake.
fn dial<T>(address: Multiaddr, config: WsConfig<T>)
    -> impl Future<Item = Either<Redirect, BytesConnection<T::Output>>, Error = Error<T::Error>>
where
    T: Transport,
    T::Output: AsyncRead + AsyncWrite
{
    trace!("dial address: {}", address);

    let WsConfig { transport, max_data_size, tls_config, .. } = config;

    let (host_port, dns_name) = match host_and_dnsname(&address) {
        Ok(x) => x,
        Err(e) => return Either::A(future::err(e))
    };

    let mut inner_addr = address.clone();

    let (use_tls, path) = match inner_addr.pop() {
        Some(Protocol::Ws(path)) => (false, path),
        Some(Protocol::Wss(path)) => {
            if dns_name.is_none() {
                debug!("no DNS name in {}", address);
                return Either::A(future::err(Error::InvalidMultiaddr(address)))
            }
            (true, path)
        }
        _ => {
            debug!("{} is not a websocket multiaddr", address);
            return Either::A(future::err(Error::InvalidMultiaddr(address)))
        }
    };

    let dial = match transport.dial(inner_addr) {
        Ok(dial) => dial,
        Err(TransportError::MultiaddrNotSupported(a)) =>
            return Either::A(future::err(Error::InvalidMultiaddr(a))),
        Err(TransportError::Other(e)) =>
            return Either::A(future::err(Error::Transport(e)))
    };

    let address1 = address.clone(); // used for logging
    let address2 = address.clone(); // used for logging
    let use_deflate = config.use_deflate;
    let future = dial.map_err(Error::Transport)
        .and_then(move |stream| {
            trace!("connected to {}", address);
            if use_tls { // begin TLS session
                let dns_name = dns_name.expect("for use_tls we have checked that dns_name is some");
                trace!("starting TLS handshake with {}", address);
                let future = tls_config.client.connect(dns_name.as_ref(), stream)
                    .map_err(move |e| {
                        debug!("TLS handshake with {} failed: {}", address, e);
                        Error::Tls(tls::Error::from(e))
                    })
                    .map(|s| EitherOutput::First(EitherOutput::First(s)));
                return Either::A(future)
            }
            // continue with plain stream
            Either::B(future::ok(EitherOutput::Second(stream)))
        })
        .and_then(move |stream| {
            trace!("sending websocket handshake request to {}", address1);
            let mut client = handshake::Client::new(host_port, path);
            if use_deflate {
                client.add_extension(Box::new(Deflate::new(Mode::Client)));
            }
            Framed::new(stream, client)
                .send(())
                .map_err(|e| Error::Handshake(Box::new(e)))
                .and_then(move |framed| {
                    trace!("awaiting websocket handshake response form {}", address2);
                    framed.into_future().map_err(|(e, _)| Error::Base(Box::new(e)))
                })
                .and_then(move |(response, framed)| {
                    match response {
                        None => {
                            debug!("connection to {} terminated during handshake", address1);
                            let e: io::Error = io::ErrorKind::ConnectionAborted.into();
                            return Err(Error::Handshake(Box::new(e)))
                        }
                        Some(Response::Redirect(r)) => {
                            debug!("received {}", r);
                            return Ok(Either::A(r))
                        }
                        Some(Response::Accepted(_)) => {
                            trace!("websocket handshake with {} successful", address1)
                        }
                    }
                    let (mut handshake, mut c) = new_connection(framed, max_data_size, Mode::Client);
                    c.add_extensions(handshake.drain_extensions());
                    Ok(Either::B(BytesConnection { inner: c }))
                })
        });

    Either::B(future)
}

// Extract host, port and optionally the DNS name from the given [`Multiaddr`].
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

// Given a location URL, build a new websocket [`Multiaddr`].
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
            let s = url.scheme();
            if s.eq_ignore_ascii_case("https") | s.eq_ignore_ascii_case("wss") {
                a.push(Protocol::Wss(url.path().into()))
            } else if s.eq_ignore_ascii_case("http") | s.eq_ignore_ascii_case("ws") {
                a.push(Protocol::Ws(url.path().into()))
            } else {
                debug!("unsupported scheme: {}", s);
                return Err(Error::InvalidRedirectLocation)
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
fn new_connection<T, C>(framed: Framed<T, C>, max_size: u64, mode: Mode) -> (C, Connection<T>)
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
    let mut conn = Connection::from_framed(framed, mode);
    conn.set_max_buffer_size(usize::try_from(max_size).unwrap_or(std::usize::MAX));
    (old.codec, conn)
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

