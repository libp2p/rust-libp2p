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

use async_tls::{client, server};
use bytes::BytesMut;
use crate::{error::Error, tls};
use either::Either;
use futures::{future::BoxFuture, prelude::*, ready, stream::BoxStream};
use libp2p_core::{
    Transport,
    either::EitherOutput,
    multiaddr::{Protocol, Multiaddr},
    transport::{ListenerEvent, TransportError}
};
use log::{debug, trace};
use soketto::{connection, data, extension::deflate::Deflate, handshake};
use std::{convert::TryInto, fmt, io, pin::Pin, task::Context, task::Poll};
use url::Url;

/// Max. number of payload bytes of a single frame.
const MAX_DATA_SIZE: usize = 256 * 1024 * 1024;

/// A Websocket transport whose output type is a [`Stream`] and [`Sink`] of
/// frame payloads which does not implement [`AsyncRead`] or
/// [`AsyncWrite`]. See [`crate::WsConfig`] if you require the latter.
#[derive(Debug, Clone)]
pub struct WsConfig<T> {
    transport: T,
    max_data_size: usize,
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
    pub fn max_data_size(&self) -> usize {
        self.max_data_size
    }

    /// Set the max. frame data size we support.
    pub fn set_max_data_size(&mut self, size: usize) -> &mut Self {
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

type TlsOrPlain<T> = EitherOutput<EitherOutput<client::TlsStream<T>, server::TlsStream<T>>, T>;

impl<T> Transport for WsConfig<T>
where
    T: Transport + Send + Clone + 'static,
    T::Error: Send + 'static,
    T::Dial: Send + 'static,
    T::Listener: Send + Unpin + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Output: AsyncRead + AsyncWrite + Unpin + Send + 'static
{
    type Output = Connection<T::Output>;
    type Error = Error<T::Error>;
    type Listener = BoxStream<'static, Result<ListenerEvent<Self::ListenerUpgrade>, Self::Error>>;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

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
        let transport = self.transport.listen_on(inner_addr).map_err(|e| e.map(Error::Transport))?;
        let listen = transport
            .map_err(Error::Transport)
            .map_ok(move |event| match event {
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

                    let upgrade = async move {
                        let stream = upgrade.map_err(Error::Transport).await?;
                        trace!("incoming connection from {}", remote1);

                        let stream =
                            if use_tls { // begin TLS session
                                let server = tls_config
                                    .server
                                    .expect("for use_tls we checked server is not none");

                                trace!("awaiting TLS handshake with {}", remote1);

                                let stream = server.accept(stream)
                                    .map_err(move |e| {
                                        debug!("TLS handshake with {} failed: {}", remote1, e);
                                        Error::Tls(tls::Error::from(e))
                                    })
                                    .await?;

                                let stream: TlsOrPlain<_> =
                                    EitherOutput::First(EitherOutput::Second(stream));

                                stream
                            } else { // continue with plain stream
                                EitherOutput::Second(stream)
                            };

                        trace!("receiving websocket handshake request from {}", remote2);

                        let mut server = handshake::Server::new(stream);

                        if use_deflate {
                            server.add_extension(Box::new(Deflate::new(connection::Mode::Server)));
                        }

                        let ws_key = {
                            let request = server.receive_request()
                                .map_err(|e| Error::Handshake(Box::new(e)))
                                .await?;
                            request.into_key()
                        };

                        trace!("accepting websocket handshake request from {}", remote2);

                        let response =
                            handshake::server::Response::Accept {
                                key: &ws_key,
                                protocol: None
                            };

                        server.send_response(&response)
                            .map_err(|e| Error::Handshake(Box::new(e)))
                            .await?;

                        let conn = {
                            let mut builder = server.into_builder();
                            builder.set_max_message_size(max_size);
                            builder.set_max_frame_size(max_size);
                            Connection::new(builder)
                        };

                        Ok(conn)
                    };

                    ListenerEvent::Upgrade {
                        upgrade: Box::pin(upgrade) as BoxFuture<'static, _>,
                        local_addr,
                        remote_addr
                    }
                }
            });
        Ok(Box::pin(listen))
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
        let mut remaining_redirects = self.max_redirects;
        let mut addr = addr;
        let future = async move {
            loop {
                let this = self.clone();
                match this.dial_once(addr).await {
                    Ok(Either::Left(redirect)) => {
                        if remaining_redirects == 0 {
                            debug!("too many redirects");
                            return Err(Error::TooManyRedirects)
                        }
                        remaining_redirects -= 1;
                        addr = location_to_multiaddr(&redirect)?
                    }
                    Ok(Either::Right(conn)) => return Ok(conn),
                    Err(e) => return Err(e)
                }
            }
        };

        Ok(Box::pin(future))
    }
}

impl<T> WsConfig<T>
where
    T: Transport,
    T::Output: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    /// Attempty to dial the given address and perform a websocket handshake.
    async fn dial_once(self, address: Multiaddr) -> Result<Either<String, Connection<T::Output>>, Error<T::Error>> {
        trace!("dial address: {}", address);

        let (host_port, dns_name) = host_and_dnsname(&address)?;

        let mut inner_addr = address.clone();

        let (use_tls, path) =
            match inner_addr.pop() {
                Some(Protocol::Ws(path)) => (false, path),
                Some(Protocol::Wss(path)) => {
                    if dns_name.is_none() {
                        debug!("no DNS name in {}", address);
                        return Err(Error::InvalidMultiaddr(address))
                    }
                    (true, path)
                }
                _ => {
                    debug!("{} is not a websocket multiaddr", address);
                    return Err(Error::InvalidMultiaddr(address))
                }
            };

        let dial = self.transport.dial(inner_addr)
            .map_err(|e| match e {
                TransportError::MultiaddrNotSupported(a) => Error::InvalidMultiaddr(a),
                TransportError::Other(e) => Error::Transport(e)
            })?;

        let stream = dial.map_err(Error::Transport).await?;
        trace!("connected to {}", address);

        let stream =
            if use_tls { // begin TLS session
                let dns_name = dns_name.expect("for use_tls we have checked that dns_name is some");
                trace!("starting TLS handshake with {}", address);
                let stream = self.tls_config.client.connect(&dns_name, stream)
                    .map_err(|e| {
                        // We should never enter here as we passed a `DNSNameRef` to `connect`.
                        debug!("invalid domain name: {:?}", dns_name);
                        Error::Tls(e.into())
                    })?
                    .map_err(|e| {
                        debug!("TLS handshake with {} failed: {}", address, e);
                        Error::Tls(tls::Error::from(e))
                    })
                    .await?;

                let stream: TlsOrPlain<_> = EitherOutput::First(EitherOutput::First(stream));
                stream
            } else { // continue with plain stream
                EitherOutput::Second(stream)
            };

        trace!("sending websocket handshake request to {}", address);

        let mut client = handshake::Client::new(stream, &host_port, path.as_ref());

        if self.use_deflate {
            client.add_extension(Box::new(Deflate::new(connection::Mode::Client)));
        }

        match client.handshake().map_err(|e| Error::Handshake(Box::new(e))).await? {
            handshake::ServerResponse::Redirect { status_code, location } => {
                debug!("received redirect ({}); location: {}", status_code, location);
                Ok(Either::Left(location))
            }
            handshake::ServerResponse::Rejected { status_code } => {
                let msg = format!("server rejected handshake; status code = {}", status_code);
                Err(Error::Handshake(msg.into()))
            }
            handshake::ServerResponse::Accepted { .. } => {
                trace!("websocket handshake with {} successful", address);
                Ok(Either::Right(Connection::new(client.into_builder())))
            }
        }
    }
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

/// The websocket connection.
pub struct Connection<T> {
    receiver: BoxStream<'static, Result<data::Incoming, connection::Error>>,
    sender: Pin<Box<dyn Sink<OutgoingData, Error = connection::Error> + Send>>,
    _marker: std::marker::PhantomData<T>
}

/// Data received over the websocket connection.
#[derive(Debug, Clone)]
pub struct IncomingData(data::Incoming);

impl IncomingData {
    pub fn is_binary(&self) -> bool {
        self.0.is_binary()
    }

    pub fn is_pong(&self) -> bool {
        self.0.is_pong()
    }
}

impl AsRef<[u8]> for IncomingData {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

/// Data sent over the websocket connection.
#[derive(Debug, Clone)]
pub enum OutgoingData {
    /// Send some bytes.
    Binary(BytesMut),
    /// Send a PING message.
    Ping(BytesMut),
    /// Send an unsolicited PONG message.
    /// (Incoming PINGs are answered automatically.)
    Pong(BytesMut)
}

impl<T> fmt::Debug for Connection<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Connection")
    }
}

impl<T> Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    fn new(builder: connection::Builder<TlsOrPlain<T>>) -> Self {
        let (sender, receiver) = builder.finish();
        let sink = quicksink::make_sink(sender, |mut sender, action| async move {
            match action {
                quicksink::Action::Send(OutgoingData::Binary(x)) => {
                    sender.send_binary_mut(x).await?
                }
                quicksink::Action::Send(OutgoingData::Ping(x)) => {
                    let data = x.as_ref().try_into().map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidInput, "PING data must be < 126 bytes")
                    })?;
                    sender.send_ping(data).await?
                }
                quicksink::Action::Send(OutgoingData::Pong(x)) => {
                    let data = x.as_ref().try_into().map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidInput, "PONG data must be < 126 bytes")
                    })?;
                    sender.send_pong(data).await?
                }
                quicksink::Action::Flush => sender.flush().await?,
                quicksink::Action::Close => sender.close().await?
            }
            Ok(sender)
        });
        Connection {
            receiver: connection::into_stream(receiver).boxed(),
            sender: Box::pin(sink),
            _marker: std::marker::PhantomData
        }
    }

    /// Send binary application data to the remote.
    pub fn send_data(&mut self, data: impl Into<BytesMut>) -> sink::Send<'_, Self, OutgoingData> {
        self.send(OutgoingData::Binary(data.into()))
    }

    /// Send a PING to the remote.
    pub fn send_ping(&mut self, data: impl Into<BytesMut>) -> sink::Send<'_, Self, OutgoingData> {
        self.send(OutgoingData::Ping(data.into()))
    }

    /// Send an unsolicited PONG to the remote.
    pub fn send_pong(&mut self, data: impl Into<BytesMut>) -> sink::Send<'_, Self, OutgoingData> {
        self.send(OutgoingData::Pong(data.into()))
    }
}

impl<T> Stream for Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    type Item = io::Result<IncomingData>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let item = ready!(self.receiver.poll_next_unpin(cx));
        let item = item.map(|result| {
            result.map(IncomingData).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        });
        Poll::Ready(item)
    }
}

impl<T> Sink<OutgoingData> for Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static
{
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.sender)
            .poll_ready(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn start_send(mut self: Pin<&mut Self>, item: OutgoingData) -> io::Result<()> {
        Pin::new(&mut self.sender)
            .start_send(item)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.sender)
            .poll_flush(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.sender)
            .poll_close(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

