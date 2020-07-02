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
//
// Copyright 2020 CoBloX Pty Ltd.

//! Implementation of the libp2p `Transport` trait for TCP/IP.
//!
//! Copied from github.com/libp2p/rust-libp2p/transports/tcp/lib.rs
//! Modified by Tobin C. Harding <tobin@coblox.tech>

use anyhow::Result;
use data_encoding::BASE32;
use futures::{
    future::{self, Ready},
    prelude::*,
};
use futures_timer::Delay;
use get_if_addrs::{get_if_addrs, IfAddr};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use libp2p::core::{
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerEvent, TransportError},
    Transport,
};
use log::{debug, info, trace};
use socket2::{Domain, Socket, Type};
use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, IpAddr},
    collections::{HashMap, VecDeque},
    convert::TryFrom,
    io,
    iter::{self, FromIterator},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};

/// Default port for the Tor SOCKS5 proxy.
const DEFAULT_SOCKS_PORT: u16 = 9050;

/// Represents the configuration for a TCP/IP transport capability for libp2p.
///
/// The TCP sockets created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through the tokio reactor.
#[cfg_attr(docsrs, doc(cfg(feature = $feature_name)))]
#[derive(Debug, Clone, Default)]
pub struct TorTokioTcpConfig {
    /// How long a listener should sleep after receiving an error, before trying again.
    sleep_on_error: Duration,
    /// TTL to set for opened sockets, or `None` to keep default.
    ttl: Option<u32>,
    /// `TCP_NODELAY` to set for opened sockets, or `None` to keep default.
    nodelay: Option<bool>,
    /// Map of Multiaddr to port number for local socket.
    onion_map: HashMap<Multiaddr, u16>,
    /// Tor SOCKS5 proxy port number.
    socks_port: u16,
}

impl TorTokioTcpConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> TorTokioTcpConfig {
        TorTokioTcpConfig {
            sleep_on_error: Duration::from_millis(100),
            ttl: None,
            nodelay: None,
            onion_map: HashMap::new(),
            socks_port: DEFAULT_SOCKS_PORT,
        }
    }

    /// Sets the TTL to set for opened sockets.
    pub fn ttl(mut self, value: u32) -> Self {
        self.ttl = Some(value);
        self
    }

    /// Sets the `TCP_NODELAY` to set for opened sockets.
    pub fn nodelay(mut self, value: bool) -> Self {
        self.nodelay = Some(value);
        self
    }

    /// Sets the map for onion address -> local socket port number.
    pub fn onion_map(mut self, value: HashMap<Multiaddr, u16>) -> Self {
        self.onion_map = value;
        self
    }

    /// Sets the Tor SOCKS5 proxy port number.
    pub fn socks_port(mut self, port: u16) -> Self {
        self.socks_port = port;
        self
    }
}

type Listener<TUpgrade, TError> =
    Pin<Box<dyn Stream<Item = Result<ListenerEvent<TUpgrade, TError>, TError>> + Send>>;

impl Transport for TorTokioTcpConfig {
    type Output = TokioTcpTransStream;
    type Error = io::Error;
    type Listener = Listener<Self::ListenerUpgrade, Self::Error>;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;
    type Dial = Pin<Box<dyn Future<Output = Result<TokioTcpTransStream, io::Error>> + Send>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let port = match self.onion_map.get(&addr) {
            Some(port) => port,
            None => return Err(TransportError::MultiaddrNotSupported(addr)),
        };
        let socket_addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, *port));

        async fn do_listen(
            cfg: TorTokioTcpConfig,
            socket_addr: SocketAddr,
        ) -> Result<
            impl Stream<
                Item = Result<
                    ListenerEvent<Ready<Result<TokioTcpTransStream, io::Error>>, io::Error>,
                    io::Error,
                >,
            >,
            io::Error,
        > {
            let socket = if socket_addr.is_ipv4() {
                Socket::new(
                    Domain::ipv4(),
                    Type::stream(),
                    Some(socket2::Protocol::tcp()),
                )?
            } else {
                let s = Socket::new(
                    Domain::ipv6(),
                    Type::stream(),
                    Some(socket2::Protocol::tcp()),
                )?;
                s.set_only_v6(true)?;
                s
            };
            if cfg!(target_family = "unix") {
                socket.set_reuse_address(true)?;
            }
            socket.bind(&socket_addr.into())?;
            socket.listen(1024)?; // we may want to make this configurable

            let listener = <TcpListener>::try_from(socket.into_tcp_listener())
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            let local_addr = listener.local_addr()?;
            let port = local_addr.port();

            // Determine all our listen addresses which is either a single local IP address
            // or (if a wildcard IP address was used) the addresses of all our interfaces,
            // as reported by `get_if_addrs`.
            let addrs = if socket_addr.ip().is_unspecified() {
                let addrs = host_addresses(port)?;
                debug!(
                    "Listening on {:?}",
                    addrs.iter().map(|(_, _, ma)| ma).collect::<Vec<_>>()
                );
                Addresses::Many(addrs)
            } else {
                let ma = ip_to_multiaddr(local_addr.ip(), port);
                debug!("Listening on {:?}", ma);
                Addresses::One(ma)
            };

            // Generate `NewAddress` events for each new `Multiaddr`.
            let pending = match addrs {
                Addresses::One(ref ma) => {
                    let event = ListenerEvent::NewAddress(ma.clone());
                    let mut list = VecDeque::new();
                    list.push_back(Ok(event));
                    list
                }
                Addresses::Many(ref aa) => aa
                    .iter()
                    .map(|(_, _, ma)| ma)
                    .cloned()
                    .map(ListenerEvent::NewAddress)
                    .map(Result::Ok)
                    .collect::<VecDeque<_>>(),
            };

            let listen_stream = TokioTcpListenStream {
                stream: listener,
                pause: None,
                pause_duration: cfg.sleep_on_error,
                port,
                addrs,
                pending,
                config: cfg,
            };

            Ok(stream::unfold(listen_stream, |s| s.next().map(Some)))
        }

        Ok(Box::pin(do_listen(self, socket_addr).try_flatten_stream()))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let dest = tor_address_string(addr.clone())
            .ok_or_else(|| TransportError::MultiaddrNotSupported(addr))?;
        debug!("dest: {}", dest);

        async fn do_dial(
            cfg: TorTokioTcpConfig,
            dest: String,
        ) -> Result<TokioTcpTransStream, io::Error> {
            info!("connecting to Tor proxy ...");
            let stream = crate::connect_tor_socks_proxy(dest, cfg.socks_port)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::ConnectionRefused, e))?;
            info!("connection established");

            apply_config(&cfg, &stream)?;

            Ok(TokioTcpTransStream { inner: stream })
        }

        Ok(Box::pin(do_dial(self, dest)))
    }
}

// Tor expects address in form: ADDR.onion:PORT
fn tor_address_string(mut multi: Multiaddr) -> Option<String> {
    let (encoded, port) = match multi.pop()? {
        Protocol::Onion(addr, port) => {
            (BASE32.encode(addr.as_ref()), port)
        }
        Protocol::Onion3(addr) => {
            (BASE32.encode(addr.hash()), addr.port())
        }
        _ => return None,
    };
    let addr = format!("{}.onion:{}", encoded.to_lowercase(), port);
    Some(addr)
}

/// Stream that listens on an TCP/IP address.
#[cfg_attr(docsrs, doc(cfg(feature = $feature_name)))]
pub struct TokioTcpListenStream {
    /// The incoming connections.
    stream: TcpListener,
    /// The current pause if any.
    pause: Option<Delay>,
    /// How long to pause after an error.
    pause_duration: Duration,
    /// The port which we use as our listen port in listener event addresses.
    port: u16,
    /// The set of known addresses.
    addrs: Addresses,
    /// Temporary buffer of listener events.
    pending: Buffer<TokioTcpTransStream>,
    /// Original configuration.
    config: TorTokioTcpConfig,
}

impl TokioTcpListenStream {
    /// Takes ownership of the listener, and returns the next incoming event and the listener.
    async fn next(
        mut self,
    ) -> (
        Result<ListenerEvent<Ready<Result<TokioTcpTransStream, io::Error>>, io::Error>, io::Error>,
        Self,
    ) {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return (event, self);
            }

            if let Some(pause) = self.pause.take() {
                let _ = pause.await;
            }

            // TODO: do we get the peer_addr at the same time?
            let (sock, _) = match self.stream.accept().await {
                Ok(s) => s,
                Err(e) => {
                    debug!("error accepting incoming connection: {}", e);
                    self.pause = Some(Delay::new(self.pause_duration));
                    return (Ok(ListenerEvent::Error(e)), self);
                }
            };

            let sock_addr = match sock.peer_addr() {
                Ok(addr) => addr,
                Err(err) => {
                    debug!("Failed to get peer address: {:?}", err);
                    continue;
                }
            };

            let local_addr = match sock.local_addr() {
                Ok(sock_addr) => {
                    if let Addresses::Many(ref mut addrs) = self.addrs {
                        if let Err(err) = check_for_interface_changes(
                            &sock_addr,
                            self.port,
                            addrs,
                            &mut self.pending,
                        ) {
                            return (Ok(ListenerEvent::Error(err)), self);
                        }
                    }
                    ip_to_multiaddr(sock_addr.ip(), sock_addr.port())
                }
                Err(err) => {
                    debug!("Failed to get local address of incoming socket: {:?}", err);
                    continue;
                }
            };

            let remote_addr = ip_to_multiaddr(sock_addr.ip(), sock_addr.port());

            match apply_config(&self.config, &sock) {
                Ok(()) => {
                    trace!("Incoming connection from {} at {}", remote_addr, local_addr);
                    self.pending.push_back(Ok(ListenerEvent::Upgrade {
                        upgrade: future::ok(TokioTcpTransStream { inner: sock }),
                        local_addr,
                        remote_addr,
                    }))
                }
                Err(err) => {
                    debug!(
                        "Error upgrading incoming connection from {}: {:?}",
                        remote_addr, err
                    );
                    self.pending.push_back(Ok(ListenerEvent::Upgrade {
                        upgrade: future::err(err),
                        local_addr,
                        remote_addr,
                    }))
                }
            }
        }
    }
}

/// Wraps around a `TcpStream` and adds logging for important events.
#[cfg_attr(docsrs, doc(cfg(feature = $feature_name)))]
#[derive(Debug)]
pub struct TokioTcpTransStream {
    inner: TcpStream,
}

impl Drop for TokioTcpTransStream {
    fn drop(&mut self) {
        if let Ok(addr) = self.inner.peer_addr() {
            debug!("Dropped TCP connection to {:?}", addr);
        } else {
            debug!("Dropped TCP connection to undeterminate peer");
        }
    }
}

/// Applies the socket configuration parameters to a socket.
fn apply_config(config: &TorTokioTcpConfig, socket: &TcpStream) -> Result<(), io::Error> {
    if let Some(ttl) = config.ttl {
        socket.set_ttl(ttl)?;
    }

    if let Some(nodelay) = config.nodelay {
        socket.set_nodelay(nodelay)?;
    }

    Ok(())
}

impl AsyncRead for TokioTcpTransStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        tokio::io::AsyncRead::poll_read(Pin::new(&mut self.inner), cx, buf)
    }
}

impl AsyncWrite for TokioTcpTransStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        tokio::io::AsyncWrite::poll_write(Pin::new(&mut self.inner), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        tokio::io::AsyncWrite::poll_flush(Pin::new(&mut self.inner), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        tokio::io::AsyncWrite::poll_shutdown(Pin::new(&mut self.inner), cx)
    }
}

// Create a [`Multiaddr`] from the given IP address and port number.
fn ip_to_multiaddr(ip: IpAddr, port: u16) -> Multiaddr {
    let proto = match ip {
        IpAddr::V4(ip) => Protocol::Ip4(ip),
        IpAddr::V6(ip) => Protocol::Ip6(ip),
    };
    let it = iter::once(proto).chain(iter::once(Protocol::Tcp(port)));
    Multiaddr::from_iter(it)
}

// Collect all local host addresses and use the provided port number as listen port.
fn host_addresses(port: u16) -> io::Result<Vec<(IpAddr, IpNet, Multiaddr)>> {
    let mut addrs = Vec::new();
    for iface in get_if_addrs()? {
        let ip = iface.ip();
        let ma = ip_to_multiaddr(ip, port);
        let ipn = match iface.addr {
            IfAddr::V4(ip4) => {
                let prefix_len = (!u32::from_be_bytes(ip4.netmask.octets())).leading_zeros();
                let ipnet = Ipv4Net::new(ip4.ip, prefix_len as u8)
                    .expect("prefix_len is the number of bits in a u32, so can not exceed 32");
                IpNet::V4(ipnet)
            }
            IfAddr::V6(ip6) => {
                let prefix_len = (!u128::from_be_bytes(ip6.netmask.octets())).leading_zeros();
                let ipnet = Ipv6Net::new(ip6.ip, prefix_len as u8)
                    .expect("prefix_len is the number of bits in a u128, so can not exceed 128");
                IpNet::V6(ipnet)
            }
        };
        addrs.push((ip, ipn, ma))
    }
    Ok(addrs)
}

/// Listen address information.
#[derive(Debug)]
enum Addresses {
    /// A specific address is used to listen.
    One(Multiaddr),
    /// A set of addresses is used to listen.
    Many(Vec<(IpAddr, IpNet, Multiaddr)>),
}

type Buffer<T> = VecDeque<Result<ListenerEvent<Ready<Result<T, io::Error>>, io::Error>, io::Error>>;

// If we listen on all interfaces, find out to which interface the given
// socket address belongs. In case we think the address is new, check
// all host interfaces again and report new and expired listen addresses.
fn check_for_interface_changes<T>(
    socket_addr: &SocketAddr,
    listen_port: u16,
    listen_addrs: &mut Vec<(IpAddr, IpNet, Multiaddr)>,
    pending: &mut Buffer<T>,
) -> Result<(), io::Error> {
    // Check for exact match:
    if listen_addrs.iter().any(|(ip, ..)| ip == &socket_addr.ip()) {
        return Ok(());
    }

    // No exact match => check netmask
    if listen_addrs
        .iter()
        .any(|(_, net, _)| net.contains(&socket_addr.ip()))
    {
        return Ok(());
    }

    // The local IP address of this socket is new to us.
    // We check for changes in the set of host addresses and report new
    // and expired addresses.
    //
    // TODO: We do not detect expired addresses unless there is a new address.
    let old_listen_addrs = std::mem::replace(listen_addrs, host_addresses(listen_port)?);

    // Check for addresses no longer in use.
    for (ip, _, ma) in old_listen_addrs.iter() {
        if listen_addrs.iter().find(|(i, ..)| i == ip).is_none() {
            debug!("Expired listen address: {}", ma);
            pending.push_back(Ok(ListenerEvent::AddressExpired(ma.clone())));
        }
    }

    // Check for new addresses.
    for (ip, _, ma) in listen_addrs.iter() {
        if old_listen_addrs.iter().find(|(i, ..)| i == ip).is_none() {
            debug!("New listen address: {}", ma);
            pending.push_back(Ok(ListenerEvent::NewAddress(ma.clone())));
        }
    }

    // We should now be able to find the local address, if not something
    // is seriously wrong and we report an error.
    if listen_addrs
        .iter()
        .find(|(ip, net, _)| ip == &socket_addr.ip() || net.contains(&socket_addr.ip()))
        .is_none()
    {
        let msg = format!("{} does not match any listen address", socket_addr.ip());
        return Err(io::Error::new(io::ErrorKind::Other, msg));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::tor_address_string;

    #[test]
    fn can_format_tor_address_v3() {
        let multi = "/onion3/vww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyyd:1234".parse().expect("failed to parse multiaddr");
        let want = "vww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyyd.onion:1234";
        let got = tor_address_string(multi).expect("failed to stringify");

        assert_eq!(got, want);
    }

    #[test]
    fn can_format_tor_address_v2() {
        let multi = "/onion/aaimaq4ygg2iegci:80".parse().expect("failed to parse multiaddr");
        let want = "aaimaq4ygg2iegci.onion:80";
        let got = tor_address_string(multi).expect("failed to stringify");

        assert_eq!(got, want);
    }
}
