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

use crate::{error::Error, tls};
use either::Either;
use futures::{future::BoxFuture, prelude::*, ready, stream::BoxStream};
use futures_rustls::{client, rustls, server};
use libp2p_core::{
    connection::Endpoint,
    either::EitherOutput,
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerId, TransportError, TransportEvent},
    Transport,
};
use log::{debug, trace};
use parking_lot::Mutex;
use soketto::{
    connection::{self, CloseReason},
    extension::deflate::Deflate,
    handshake,
};
use std::{collections::HashMap, ops::DerefMut, sync::Arc};
use std::{convert::TryInto, fmt, io, mem, pin::Pin, task::Context, task::Poll};
use url::Url;

/// Max. number of payload bytes of a single frame.
const MAX_DATA_SIZE: usize = 256 * 1024 * 1024;

/// A Websocket transport whose output type is a [`Stream`] and [`Sink`] of
/// frame payloads which does not implement [`AsyncRead`] or
/// [`AsyncWrite`]. See [`crate::WsConfig`] if you require the latter.
#[derive(Debug)]
pub struct WsConfig<T> {
    transport: Arc<Mutex<T>>,
    max_data_size: usize,
    tls_config: tls::Config,
    max_redirects: u8,
    use_deflate: bool,
    /// Websocket protocol of the inner listener.
    ///
    /// This is the suffix of the address provided in `listen_on`.
    /// Can only be [`Protocol::Ws`] or [`Protocol::Wss`].
    listener_protos: HashMap<ListenerId, Protocol<'static>>,
}

impl<T> WsConfig<T> {
    /// Create a new websocket transport based on another transport.
    pub fn new(transport: T) -> Self {
        WsConfig {
            transport: Arc::new(Mutex::new(transport)),
            max_data_size: MAX_DATA_SIZE,
            tls_config: tls::Config::client(),
            max_redirects: 0,
            use_deflate: false,
            listener_protos: HashMap::new(),
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
    T: Transport + Send + Unpin + 'static,
    T::Error: Send + 'static,
    T::Dial: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Output: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = Connection<T::Output>;
    type Error = Error<T::Error>;
    type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
    type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<Self::Error>> {
        let mut inner_addr = addr.clone();
        let proto = match inner_addr.pop() {
            Some(p @ Protocol::Wss(_)) => {
                if self.tls_config.server.is_some() {
                    p
                } else {
                    debug!("/wss address but TLS server support is not configured");
                    return Err(TransportError::MultiaddrNotSupported(addr));
                }
            }
            Some(p @ Protocol::Ws(_)) => p,
            _ => {
                debug!("{} is not a websocket multiaddr", addr);
                return Err(TransportError::MultiaddrNotSupported(addr));
            }
        };
        match self.transport.lock().listen_on(inner_addr) {
            Ok(id) => {
                self.listener_protos.insert(id, proto);
                Ok(id)
            }
            Err(e) => Err(e.map(Error::Transport)),
        }
    }

    fn remove_listener(&mut self, id: ListenerId) -> bool {
        self.transport.lock().remove_listener(id)
    }

    fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.do_dial(addr, Endpoint::Dialer)
    }

    fn dial_as_listener(
        &mut self,
        addr: Multiaddr,
    ) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.do_dial(addr, Endpoint::Listener)
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.lock().address_translation(server, observed)
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<libp2p_core::transport::TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        let inner_event = {
            let mut transport = self.transport.lock();
            match Transport::poll(Pin::new(transport.deref_mut()), cx) {
                Poll::Ready(ev) => ev,
                Poll::Pending => return Poll::Pending,
            }
        };
        let event = match inner_event {
            TransportEvent::NewAddress {
                listener_id,
                mut listen_addr,
            } => {
                // Append the ws / wss protocol back to the inner address.
                let proto = self
                    .listener_protos
                    .get(&listener_id)
                    .expect("Protocol was inserted in Transport::listen_on.");
                listen_addr.push(proto.clone());
                debug!("Listening on {}", listen_addr);
                TransportEvent::NewAddress {
                    listener_id,
                    listen_addr,
                }
            }
            TransportEvent::AddressExpired {
                listener_id,
                mut listen_addr,
            } => {
                let proto = self
                    .listener_protos
                    .get(&listener_id)
                    .expect("Protocol was inserted in Transport::listen_on.");
                listen_addr.push(proto.clone());
                TransportEvent::AddressExpired {
                    listener_id,
                    listen_addr,
                }
            }
            TransportEvent::ListenerError { listener_id, error } => TransportEvent::ListenerError {
                listener_id,
                error: Error::Transport(error),
            },
            TransportEvent::ListenerClosed {
                listener_id,
                reason,
            } => {
                self.listener_protos
                    .remove(&listener_id)
                    .expect("Protocol was inserted in Transport::listen_on.");
                TransportEvent::ListenerClosed {
                    listener_id,
                    reason: reason.map_err(Error::Transport),
                }
            }
            TransportEvent::Incoming {
                listener_id,
                upgrade,
                mut local_addr,
                mut send_back_addr,
            } => {
                let proto = self
                    .listener_protos
                    .get(&listener_id)
                    .expect("Protocol was inserted in Transport::listen_on.");
                let use_tls = match proto {
                    Protocol::Wss(_) => true,
                    Protocol::Ws(_) => false,
                    _ => unreachable!("Map contains only ws and wss protocols."),
                };
                local_addr.push(proto.clone());
                send_back_addr.push(proto.clone());
                let upgrade = self.map_upgrade(upgrade, send_back_addr.clone(), use_tls);
                TransportEvent::Incoming {
                    listener_id,
                    upgrade,
                    local_addr,
                    send_back_addr,
                }
            }
        };
        Poll::Ready(event)
    }
}

impl<T> WsConfig<T>
where
    T: Transport + Send + Unpin + 'static,
    T::Error: Send + 'static,
    T::Dial: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Output: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    fn do_dial(
        &mut self,
        addr: Multiaddr,
        role_override: Endpoint,
    ) -> Result<<Self as Transport>::Dial, TransportError<<Self as Transport>::Error>> {
        let mut addr = match parse_ws_dial_addr(addr) {
            Ok(addr) => addr,
            Err(Error::InvalidMultiaddr(a)) => {
                return Err(TransportError::MultiaddrNotSupported(a))
            }
            Err(e) => return Err(TransportError::Other(e)),
        };

        // We are looping here in order to follow redirects (if any):
        let mut remaining_redirects = self.max_redirects;

        let transport = self.transport.clone();
        let tls_config = self.tls_config.clone();
        let use_deflate = self.use_deflate;
        let max_redirects = self.max_redirects;

        let future = async move {
            loop {
                match Self::dial_once(
                    transport.clone(),
                    addr,
                    tls_config.clone(),
                    use_deflate,
                    role_override,
                )
                .await
                {
                    Ok(Either::Left(redirect)) => {
                        if remaining_redirects == 0 {
                            debug!("Too many redirects (> {})", max_redirects);
                            return Err(Error::TooManyRedirects);
                        }
                        remaining_redirects -= 1;
                        addr = parse_ws_dial_addr(location_to_multiaddr(&redirect)?)?
                    }
                    Ok(Either::Right(conn)) => return Ok(conn),
                    Err(e) => return Err(e),
                }
            }
        };

        Ok(Box::pin(future))
    }

    /// Attempts to dial the given address and perform a websocket handshake.
    async fn dial_once(
        transport: Arc<Mutex<T>>,
        addr: WsAddress,
        tls_config: tls::Config,
        use_deflate: bool,
        role_override: Endpoint,
    ) -> Result<Either<String, Connection<T::Output>>, Error<T::Error>> {
        trace!("Dialing websocket address: {:?}", addr);

        let dial = match role_override {
            Endpoint::Dialer => transport.lock().dial(addr.tcp_addr),
            Endpoint::Listener => transport.lock().dial_as_listener(addr.tcp_addr),
        }
        .map_err(|e| match e {
            TransportError::MultiaddrNotSupported(a) => Error::InvalidMultiaddr(a),
            TransportError::Other(e) => Error::Transport(e),
        })?;

        let stream = dial.map_err(Error::Transport).await?;
        trace!("TCP connection to {} established.", addr.host_port);

        let stream = if addr.use_tls {
            // begin TLS session
            let dns_name = addr
                .dns_name
                .expect("for use_tls we have checked that dns_name is some");
            trace!("Starting TLS handshake with {:?}", dns_name);
            let stream = tls_config
                .client
                .connect(dns_name.clone(), stream)
                .map_err(|e| {
                    debug!("TLS handshake with {:?} failed: {}", dns_name, e);
                    Error::Tls(tls::Error::from(e))
                })
                .await?;

            let stream: TlsOrPlain<_> = EitherOutput::First(EitherOutput::First(stream));
            stream
        } else {
            // continue with plain stream
            EitherOutput::Second(stream)
        };

        trace!("Sending websocket handshake to {}", addr.host_port);

        let mut client = handshake::Client::new(stream, &addr.host_port, addr.path.as_ref());

        if use_deflate {
            client.add_extension(Box::new(Deflate::new(connection::Mode::Client)));
        }

        match client
            .handshake()
            .map_err(|e| Error::Handshake(Box::new(e)))
            .await?
        {
            handshake::ServerResponse::Redirect {
                status_code,
                location,
            } => {
                debug!(
                    "received redirect ({}); location: {}",
                    status_code, location
                );
                Ok(Either::Left(location))
            }
            handshake::ServerResponse::Rejected { status_code } => {
                let msg = format!("server rejected handshake; status code = {}", status_code);
                Err(Error::Handshake(msg.into()))
            }
            handshake::ServerResponse::Accepted { .. } => {
                trace!("websocket handshake with {} successful", addr.host_port);
                Ok(Either::Right(Connection::new(client.into_builder())))
            }
        }
    }

    fn map_upgrade(
        &self,
        upgrade: T::ListenerUpgrade,
        remote_addr: Multiaddr,
        use_tls: bool,
    ) -> <Self as Transport>::ListenerUpgrade {
        let remote_addr2 = remote_addr.clone(); // used for logging
        let tls_config = self.tls_config.clone();
        let max_size = self.max_data_size;
        let use_deflate = self.use_deflate;

        async move {
            let stream = upgrade.map_err(Error::Transport).await?;
            trace!("incoming connection from {}", remote_addr);

            let stream = if use_tls {
                // begin TLS session
                let server = tls_config
                    .server
                    .expect("for use_tls we checked server is not none");

                trace!("awaiting TLS handshake with {}", remote_addr);

                let stream = server
                    .accept(stream)
                    .map_err(move |e| {
                        debug!("TLS handshake with {} failed: {}", remote_addr, e);
                        Error::Tls(tls::Error::from(e))
                    })
                    .await?;

                let stream: TlsOrPlain<_> = EitherOutput::First(EitherOutput::Second(stream));

                stream
            } else {
                // continue with plain stream
                EitherOutput::Second(stream)
            };

            trace!(
                "receiving websocket handshake request from {}",
                remote_addr2
            );

            let mut server = handshake::Server::new(stream);

            if use_deflate {
                server.add_extension(Box::new(Deflate::new(connection::Mode::Server)));
            }

            let ws_key = {
                let request = server
                    .receive_request()
                    .map_err(|e| Error::Handshake(Box::new(e)))
                    .await?;
                request.key()
            };

            trace!(
                "accepting websocket handshake request from {}",
                remote_addr2
            );

            let response = handshake::server::Response::Accept {
                key: ws_key,
                protocol: None,
            };

            server
                .send_response(&response)
                .map_err(|e| Error::Handshake(Box::new(e)))
                .await?;

            let conn = {
                let mut builder = server.into_builder();
                builder.set_max_message_size(max_size);
                builder.set_max_frame_size(max_size);
                Connection::new(builder)
            };

            Ok(conn)
        }
        .boxed()
    }
}

#[derive(Debug)]
struct WsAddress {
    host_port: String,
    path: String,
    dns_name: Option<rustls::ServerName>,
    use_tls: bool,
    tcp_addr: Multiaddr,
}

/// Tries to parse the given `Multiaddr` into a `WsAddress` used
/// for dialing.
///
/// Fails if the given `Multiaddr` does not represent a TCP/IP-based
/// websocket protocol stack.
fn parse_ws_dial_addr<T>(addr: Multiaddr) -> Result<WsAddress, Error<T>> {
    // The encapsulating protocol must be based on TCP/IP, possibly via DNS.
    // We peek at it in order to learn the hostname and port to use for
    // the websocket handshake.
    let mut protocols = addr.iter();
    let mut ip = protocols.next();
    let mut tcp = protocols.next();
    let (host_port, dns_name) = loop {
        match (ip, tcp) {
            (Some(Protocol::Ip4(ip)), Some(Protocol::Tcp(port))) => {
                break (format!("{}:{}", ip, port), None)
            }
            (Some(Protocol::Ip6(ip)), Some(Protocol::Tcp(port))) => {
                break (format!("{}:{}", ip, port), None)
            }
            (Some(Protocol::Dns(h)), Some(Protocol::Tcp(port)))
            | (Some(Protocol::Dns4(h)), Some(Protocol::Tcp(port)))
            | (Some(Protocol::Dns6(h)), Some(Protocol::Tcp(port)))
            | (Some(Protocol::Dnsaddr(h)), Some(Protocol::Tcp(port))) => {
                break (format!("{}:{}", &h, port), Some(tls::dns_name_ref(&h)?))
            }
            (Some(_), Some(p)) => {
                ip = Some(p);
                tcp = protocols.next();
            }
            _ => return Err(Error::InvalidMultiaddr(addr)),
        }
    };

    // Now consume the `Ws` / `Wss` protocol from the end of the address,
    // preserving the trailing `P2p` protocol that identifies the remote,
    // if any.
    let mut protocols = addr.clone();
    let mut p2p = None;
    let (use_tls, path) = loop {
        match protocols.pop() {
            p @ Some(Protocol::P2p(_)) => p2p = p,
            Some(Protocol::Ws(path)) => break (false, path.into_owned()),
            Some(Protocol::Wss(path)) => {
                if dns_name.is_none() {
                    debug!("Missing DNS name in WSS address: {}", addr);
                    return Err(Error::InvalidMultiaddr(addr));
                }
                break (true, path.into_owned());
            }
            _ => return Err(Error::InvalidMultiaddr(addr)),
        }
    };

    // The original address, stripped of the `/ws` and `/wss` protocols,
    // makes up the the address for the inner TCP-based transport.
    let tcp_addr = match p2p {
        Some(p) => protocols.with(p),
        None => protocols,
    };

    Ok(WsAddress {
        host_port,
        dns_name,
        path,
        use_tls,
        tcp_addr,
    })
}

// Given a location URL, build a new websocket [`Multiaddr`].
fn location_to_multiaddr<T>(location: &str) -> Result<Multiaddr, Error<T>> {
    match Url::parse(location) {
        Ok(url) => {
            let mut a = Multiaddr::empty();
            match url.host() {
                Some(url::Host::Domain(h)) => a.push(Protocol::Dns(h.into())),
                Some(url::Host::Ipv4(ip)) => a.push(Protocol::Ip4(ip)),
                Some(url::Host::Ipv6(ip)) => a.push(Protocol::Ip6(ip)),
                None => return Err(Error::InvalidRedirectLocation),
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
                return Err(Error::InvalidRedirectLocation);
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
    receiver: BoxStream<'static, Result<Incoming, connection::Error>>,
    sender: Pin<Box<dyn Sink<OutgoingData, Error = connection::Error> + Send>>,
    _marker: std::marker::PhantomData<T>,
}

/// Data or control information received over the websocket connection.
#[derive(Debug, Clone)]
pub enum Incoming {
    /// Application data.
    Data(Data),
    /// PONG control frame data.
    Pong(Vec<u8>),
    /// Close reason.
    Closed(CloseReason),
}

/// Application data received over the websocket connection
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Data {
    /// UTF-8 encoded textual data.
    Text(Vec<u8>),
    /// Binary data.
    Binary(Vec<u8>),
}

impl Data {
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Data::Text(d) => d,
            Data::Binary(d) => d,
        }
    }
}

impl AsRef<[u8]> for Data {
    fn as_ref(&self) -> &[u8] {
        match self {
            Data::Text(d) => d,
            Data::Binary(d) => d,
        }
    }
}

impl Incoming {
    pub fn is_data(&self) -> bool {
        self.is_binary() || self.is_text()
    }

    pub fn is_binary(&self) -> bool {
        matches!(self, Incoming::Data(Data::Binary(_)))
    }

    pub fn is_text(&self) -> bool {
        matches!(self, Incoming::Data(Data::Text(_)))
    }

    pub fn is_pong(&self) -> bool {
        matches!(self, Incoming::Pong(_))
    }

    pub fn is_close(&self) -> bool {
        matches!(self, Incoming::Closed(_))
    }
}

/// Data sent over the websocket connection.
#[derive(Debug, Clone)]
pub enum OutgoingData {
    /// Send some bytes.
    Binary(Vec<u8>),
    /// Send a PING message.
    Ping(Vec<u8>),
    /// Send an unsolicited PONG message.
    /// (Incoming PINGs are answered automatically.)
    Pong(Vec<u8>),
}

impl<T> fmt::Debug for Connection<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Connection")
    }
}

impl<T> Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    fn new(builder: connection::Builder<TlsOrPlain<T>>) -> Self {
        let (sender, receiver) = builder.finish();
        let sink = quicksink::make_sink(sender, |mut sender, action| async move {
            match action {
                quicksink::Action::Send(OutgoingData::Binary(x)) => {
                    sender.send_binary_mut(x).await?
                }
                quicksink::Action::Send(OutgoingData::Ping(x)) => {
                    let data = x[..].try_into().map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidInput, "PING data must be < 126 bytes")
                    })?;
                    sender.send_ping(data).await?
                }
                quicksink::Action::Send(OutgoingData::Pong(x)) => {
                    let data = x[..].try_into().map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidInput, "PONG data must be < 126 bytes")
                    })?;
                    sender.send_pong(data).await?
                }
                quicksink::Action::Flush => sender.flush().await?,
                quicksink::Action::Close => sender.close().await?,
            }
            Ok(sender)
        });
        let stream = stream::unfold((Vec::new(), receiver), |(mut data, mut receiver)| async {
            match receiver.receive(&mut data).await {
                Ok(soketto::Incoming::Data(soketto::Data::Text(_))) => Some((
                    Ok(Incoming::Data(Data::Text(mem::take(&mut data)))),
                    (data, receiver),
                )),
                Ok(soketto::Incoming::Data(soketto::Data::Binary(_))) => Some((
                    Ok(Incoming::Data(Data::Binary(mem::take(&mut data)))),
                    (data, receiver),
                )),
                Ok(soketto::Incoming::Pong(pong)) => {
                    Some((Ok(Incoming::Pong(Vec::from(pong))), (data, receiver)))
                }
                Ok(soketto::Incoming::Closed(reason)) => {
                    Some((Ok(Incoming::Closed(reason)), (data, receiver)))
                }
                Err(connection::Error::Closed) => None,
                Err(e) => Some((Err(e), (data, receiver))),
            }
        });
        Connection {
            receiver: stream.boxed(),
            sender: Box::pin(sink),
            _marker: std::marker::PhantomData,
        }
    }

    /// Send binary application data to the remote.
    pub fn send_data(&mut self, data: Vec<u8>) -> sink::Send<'_, Self, OutgoingData> {
        self.send(OutgoingData::Binary(data))
    }

    /// Send a PING to the remote.
    pub fn send_ping(&mut self, data: Vec<u8>) -> sink::Send<'_, Self, OutgoingData> {
        self.send(OutgoingData::Ping(data))
    }

    /// Send an unsolicited PONG to the remote.
    pub fn send_pong(&mut self, data: Vec<u8>) -> sink::Send<'_, Self, OutgoingData> {
        self.send(OutgoingData::Pong(data))
    }
}

impl<T> Stream for Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Item = io::Result<Incoming>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let item = ready!(self.receiver.poll_next_unpin(cx));
        let item = item.map(|result| result.map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
        Poll::Ready(item)
    }
}

impl<T> Sink<OutgoingData> for Connection<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.sender)
            .poll_ready(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn start_send(mut self: Pin<&mut Self>, item: OutgoingData) -> io::Result<()> {
        Pin::new(&mut self.sender)
            .start_send(item)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.sender)
            .poll_flush(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.sender)
            .poll_close(cx)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}
