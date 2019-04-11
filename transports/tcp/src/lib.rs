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

//! Implementation of the libp2p `Transport` trait for TCP/IP.
//!
//! Uses [the *tokio* library](https://tokio.rs).
//!
//! # Usage
//!
//! Example:
//!
//! ```
//! extern crate libp2p_tcp;
//! use libp2p_tcp::TcpConfig;
//!
//! # fn main() {
//! let tcp = TcpConfig::new();
//! # }
//! ```
//!
//! The `TcpConfig` structs implements the `Transport` trait of the `swarm` library. See the
//! documentation of `swarm` and of libp2p in general to learn how to use the `Transport` trait.

use futures::{
    future::{self, Either, FutureResult},
    prelude::*,
    stream::{self, Chain, IterOk, Once}
};
use get_if_addrs::get_if_addrs;
use libp2p_core::{Transport, transport::{ListenerEvent, TransportError}};
use log::{debug, error};
use multiaddr::{Protocol, Multiaddr};
use std::{
    collections::{HashMap, VecDeque},
    fmt,
    io::{self, Read, Write},
    iter::{self, FromIterator},
    net::{IpAddr, SocketAddr},
    time::Duration,
    vec::IntoIter
};
use tk_listen::{ListenExt, SleepOnError};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tcp::{ConnectFuture, Incoming, TcpStream};

/// Represents the configuration for a TCP/IP transport capability for libp2p.
///
/// The TCP sockets created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through the tokio reactor.
#[derive(Debug, Clone, Default)]
pub struct TcpConfig {
    /// How long a listener should sleep after receiving an error, before trying again.
    sleep_on_error: Duration,
    /// Size of the recv buffer size to set for opened sockets, or `None` to keep default.
    recv_buffer_size: Option<usize>,
    /// Size of the send buffer size to set for opened sockets, or `None` to keep default.
    send_buffer_size: Option<usize>,
    /// TTL to set for opened sockets, or `None` to keep default.
    ttl: Option<u32>,
    /// Keep alive duration to set for opened sockets, or `None` to keep default.
    keepalive: Option<Option<Duration>>,
    /// `TCP_NODELAY` to set for opened sockets, or `None` to keep default.
    nodelay: Option<bool>,
}

impl TcpConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> TcpConfig {
        TcpConfig {
            sleep_on_error: Duration::from_millis(100),
            recv_buffer_size: None,
            send_buffer_size: None,
            ttl: None,
            keepalive: None,
            nodelay: None,
        }
    }

    /// Sets the size of the recv buffer size to set for opened sockets.
    pub fn recv_buffer_size(mut self, value: usize) -> Self {
        self.recv_buffer_size = Some(value);
        self
    }

    /// Sets the size of the send buffer size to set for opened sockets.
    pub fn send_buffer_size(mut self, value: usize) -> Self {
        self.send_buffer_size = Some(value);
        self
    }

    /// Sets the TTL to set for opened sockets.
    pub fn ttl(mut self, value: u32) -> Self {
        self.ttl = Some(value);
        self
    }

    /// Sets the keep alive pinging duration to set for opened sockets.
    pub fn keepalive(mut self, value: Option<Duration>) -> Self {
        self.keepalive = Some(value);
        self
    }

    /// Sets the `TCP_NODELAY` to set for opened sockets.
    pub fn nodelay(mut self, value: bool) -> Self {
        self.nodelay = Some(value);
        self
    }
}

impl Transport for TcpConfig {
    type Output = TcpTransStream;
    type Error = io::Error;
    type Listener = TcpListener;
    type ListenerUpgrade = FutureResult<Self::Output, Self::Error>;
    type Dial = TcpDialFut;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let socket_addr =
            if let Ok(sa) = multiaddr_to_socketaddr(&addr) {
                sa
            } else {
                return Err(TransportError::MultiaddrNotSupported(addr))
            };

        let listener = tokio_tcp::TcpListener::bind(&socket_addr).map_err(TransportError::Other)?;
        let local_addr = listener.local_addr().map_err(TransportError::Other)?;
        let port = local_addr.port();

        // Determine all our listen addresses which is either a single local IP address
        // or (if a wildcard IP address was used) the addresses of all our interfaces,
        // as reported by `get_if_addrs`.
        let addrs =
            if socket_addr.ip().is_unspecified() {
                let addrs = host_addresses(port).map_err(TransportError::Other)?;
                debug!("Listening on {:?}", addrs.values());
                Addresses::Many(addrs)
            } else {
                let ma = sockaddr_to_multiaddr(local_addr.ip(), port);
                debug!("Listening on {:?}", ma);
                Addresses::One(ma)
            };

        // Generate `NewAddress` events for each new `Multiaddr`.
        let events = match addrs {
            Addresses::One(ref ma) => {
                let event = ListenerEvent::NewAddress(ma.clone());
                Either::A(stream::once(Ok(event)))
            }
            Addresses::Many(ref aa) => {
                let events = aa.values()
                    .cloned()
                    .map(ListenerEvent::NewAddress)
                    .collect::<Vec<_>>();
                Either::B(stream::iter_ok(events))
            }
        };

        let stream = TcpListenStream {
            inner: Ok(listener.incoming().sleep_on_error(self.sleep_on_error)),
            port,
            addrs,
            pending: VecDeque::new(),
            config: self
        };

        Ok(TcpListener {
            inner: match events {
                Either::A(e) => Either::A(e.chain(stream)),
                Either::B(e) => Either::B(e.chain(stream))
            }
        })
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr =
            if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
                if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                    debug!("Instantly refusing dialing {}, as it is invalid", addr);
                    return Err(TransportError::Other(io::ErrorKind::ConnectionRefused.into()))
                }
                socket_addr
            } else {
                return Err(TransportError::MultiaddrNotSupported(addr))
            };

        debug!("Dialing {}", addr);

        let future = TcpDialFut {
            inner: TcpStream::connect(&socket_addr),
            config: self
        };

        Ok(future)
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        let mut address = Multiaddr::empty();

        // Use the observed IP address.
        match server.iter().zip(observed.iter()).next() {
            Some((Protocol::Ip4(_), x @ Protocol::Ip4(_))) => address.append(x),
            Some((Protocol::Ip6(_), x @ Protocol::Ip4(_))) => address.append(x),
            Some((Protocol::Ip4(_), x @ Protocol::Ip6(_))) => address.append(x),
            Some((Protocol::Ip6(_), x @ Protocol::Ip6(_))) => address.append(x),
            _ => return None
        }

        // Carry over everything else from the server address.
        for proto in server.iter().skip(1) {
            address.append(proto)
        }

        Some(address)
    }
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Result<SocketAddr, ()> {
    let mut iter = addr.iter();
    let proto1 = iter.next().ok_or(())?;
    let proto2 = iter.next().ok_or(())?;

    if iter.next().is_some() {
        return Err(());
    }

    match (proto1, proto2) {
        (Protocol::Ip4(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        (Protocol::Ip6(ip), Protocol::Tcp(port)) => Ok(SocketAddr::new(ip.into(), port)),
        _ => Err(()),
    }
}

// Create a [`Multiaddr`] from the given IP address and port number.
fn sockaddr_to_multiaddr(ip: IpAddr, port: u16) -> Multiaddr {
    let proto = match ip {
        IpAddr::V4(ip) => Protocol::Ip4(ip),
        IpAddr::V6(ip) => Protocol::Ip6(ip)
    };
    let it = iter::once(proto).chain(iter::once(Protocol::Tcp(port)));
    Multiaddr::from_iter(it)
}

// Collect all local host addresses and use the provided port number as listen port.
fn host_addresses(port: u16) -> io::Result<HashMap<IpAddr, Multiaddr>> {
    let mut addrs = HashMap::new();
    for iface in get_if_addrs()? {
        let ma = sockaddr_to_multiaddr(iface.ip(), port);
        addrs.insert(iface.ip(), ma);
    }
    Ok(addrs)
}

/// Applies the socket configuration parameters to a socket.
fn apply_config(config: &TcpConfig, socket: &TcpStream) -> Result<(), io::Error> {
    if let Some(recv_buffer_size) = config.recv_buffer_size {
        socket.set_recv_buffer_size(recv_buffer_size)?;
    }

    if let Some(send_buffer_size) = config.send_buffer_size {
        socket.set_send_buffer_size(send_buffer_size)?;
    }

    if let Some(ttl) = config.ttl {
        socket.set_ttl(ttl)?;
    }

    if let Some(keepalive) = config.keepalive {
        socket.set_keepalive(keepalive)?;
    }

    if let Some(nodelay) = config.nodelay {
        socket.set_nodelay(nodelay)?;
    }

    Ok(())
}

/// Future that dials a TCP/IP address.
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct TcpDialFut {
    inner: ConnectFuture,
    /// Original configuration.
    config: TcpConfig,
}

impl Future for TcpDialFut {
    type Item = TcpTransStream;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<TcpTransStream, io::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(stream)) => {
                apply_config(&self.config, &stream)?;
                Ok(Async::Ready(TcpTransStream { inner: stream }))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => {
                debug!("Error while dialing => {:?}", err);
                Err(err)
            }
        }
    }
}

/// Stream of `ListenerEvent`s.
#[derive(Debug)]
pub struct TcpListener {
    inner: Either<
        Chain<Once<ListenerEvent<FutureResult<TcpTransStream, io::Error>>, io::Error>, TcpListenStream>,
        Chain<IterOk<IntoIter<ListenerEvent<FutureResult<TcpTransStream, io::Error>>>, io::Error>, TcpListenStream>
    >
}

impl Stream for TcpListener {
    type Item = ListenerEvent<FutureResult<TcpTransStream, io::Error>>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner {
            Either::A(ref mut it) => it.poll(),
            Either::B(ref mut it) => it.poll()
        }
    }
}

/// Listen address information.
#[derive(Debug)]
enum Addresses {
    /// A specific address is used to listen.
    One(Multiaddr),
    /// A set of addresses is used to listen.
    Many(HashMap<IpAddr, Multiaddr>)
}

/// Stream that listens on an TCP/IP address.
pub struct TcpListenStream {
    /// Stream of incoming sockets.
    inner: Result<SleepOnError<Incoming>, Option<io::Error>>,
    /// The port which we use as our listen port in listener event addresses.
    port: u16,
    /// The set of known addresses.
    addrs: Addresses,
    /// Temporary buffer of listener events.
    pending: VecDeque<ListenerEvent<FutureResult<TcpTransStream, io::Error>>>,
    /// Original configuration.
    config: TcpConfig
}

impl Stream for TcpListenStream {
    type Item = ListenerEvent<FutureResult<TcpTransStream, io::Error>>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, io::Error> {
        let inner = match self.inner {
            Ok(ref mut inc) => inc,
            Err(ref mut err) => return Err(err.take().expect("poll called again after error"))
        };

        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Async::Ready(Some(event)))
            }

            let sock = match inner.poll() {
                Ok(Async::Ready(Some(sock))) => sock,
                Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(()) => unreachable!("sleep_on_error never produces an error")
            };

            let sock_addr = match sock.peer_addr() {
                Ok(addr) => addr,
                Err(err) => {
                    error!("Failed to get peer address: {:?}", err);
                    return Err(err)
                }
            };

            let listen_addr = match sock.local_addr() {
                Ok(addr) => match self.addrs {
                    Addresses::One(ref ma) => ma.clone(),
                    Addresses::Many(ref mut addrs) => if let Some(ma) = addrs.get(&addr.ip()) {
                        ma.clone()
                    } else {
                        // The local IP address of this socket is new to us.
                        // We need to check for changes in the set of host addresses and report
                        // new and expired addresses.
                        //
                        // TODO: We do not detect expired addresses unless there is a new address.

                        let new_addrs = host_addresses(self.port)?;
                        let old_addrs = std::mem::replace(addrs, new_addrs);

                        // Check for addresses no longer in use.
                        for (ip, ma) in &old_addrs {
                            if !addrs.contains_key(&ip) {
                                debug!("Expired listen address: {}", ma);
                                self.pending.push_back(ListenerEvent::AddressExpired(ma.clone()));
                            }
                        }

                        // Check for new addresses.
                        for (ip, ma) in addrs {
                            if !old_addrs.contains_key(&ip) {
                                debug!("New listen address: {}", ma);
                                self.pending.push_back(ListenerEvent::NewAddress(ma.clone()));
                            }
                        }

                        sockaddr_to_multiaddr(addr.ip(), self.port)
                    }
                }
                Err(err) => {
                    error!("Failed to get local address of incoming socket: {:?}", err);
                    return Err(err)
                }
            };

            let remote_addr = sockaddr_to_multiaddr(sock_addr.ip(), sock_addr.port());

            match apply_config(&self.config, &sock) {
                Ok(()) => {
                    debug!("Incoming connection from {}", remote_addr);
                    self.pending.push_back(ListenerEvent::Upgrade {
                        upgrade: future::ok(TcpTransStream { inner: sock }),
                        listen_addr,
                        remote_addr
                    })
                }
                Err(err) => {
                    self.pending.push_back(ListenerEvent::Upgrade {
                        upgrade: future::err(err),
                        listen_addr,
                        remote_addr
                    })
                }
            }
        }
    }
}

impl fmt::Debug for TcpListenStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.inner {
            Ok(_) => write!(f, "TcpListenStream"),
            Err(None) => write!(f, "TcpListenStream(Errored)"),
            Err(Some(ref err)) => write!(f, "TcpListenStream({:?})", err),
        }
    }
}

/// Wraps around a `TcpStream` and adds logging for important events.
#[derive(Debug)]
pub struct TcpTransStream {
    inner: TcpStream,
}

impl Read for TcpTransStream {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.inner.read(buf)
    }
}

impl AsyncRead for TcpTransStream {}

impl Write for TcpTransStream {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.inner.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> Result<(), io::Error> {
        self.inner.flush()
    }
}

impl AsyncWrite for TcpTransStream {
    #[inline]
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        AsyncWrite::shutdown(&mut self.inner)
    }
}

impl Drop for TcpTransStream {
    #[inline]
    fn drop(&mut self) {
        if let Ok(addr) = self.inner.peer_addr() {
            debug!("Dropped TCP connection to {:?}", addr);
        } else {
            debug!("Dropped TCP connection to undeterminate peer");
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::runtime::current_thread::Runtime;
    use super::{multiaddr_to_socketaddr, TcpConfig};
    use futures::stream::Stream;
    use futures::Future;
    use multiaddr::{Multiaddr, Protocol};
    use std;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use libp2p_core::{Transport, transport::ListenerEvent};
    use tokio_io;

    #[test]
    fn wildcard_expansion() {
        let mut listener = TcpConfig::new()
            .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
            .expect("listener");

        // Get the first address.
        let addr = listener.by_ref()
            .wait()
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        // Process all initial `NewAddress` events and make sure they
        // do not contain wildcard address or port.
        let server = listener
            .take_while(|event| match event {
                ListenerEvent::NewAddress(a) => {
                    let mut iter = a.iter();
                    match iter.next().expect("ip address") {
                        Protocol::Ip4(ip) => assert!(!ip.is_unspecified()),
                        Protocol::Ip6(ip) => assert!(!ip.is_unspecified()),
                        other => panic!("Unexpected protocol: {}", other)
                    }
                    if let Protocol::Tcp(port) = iter.next().expect("port") {
                        assert_ne!(0, port)
                    } else {
                        panic!("No TCP port in address: {}", a)
                    }
                    Ok(true)
                }
                _ => Ok(false)
            })
            .for_each(|_| Ok(()));

        let client = TcpConfig::new().dial(addr).expect("dialer");
        tokio::run(server.join(client).map(|_| ()).map_err(|e| panic!("error: {}", e)))
    }

    #[test]
    fn multiaddr_to_tcp_conversion() {
        use std::net::Ipv6Addr;

        assert!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap())
                .is_err()
        );

        assert_eq!(
            multiaddr_to_socketaddr(&"/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                12345,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip4/255.255.255.255/tcp/8080"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
                8080,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(&"/ip6/::1/tcp/12345".parse::<Multiaddr>().unwrap()),
            Ok(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
                12345,
            ))
        );
        assert_eq!(
            multiaddr_to_socketaddr(
                &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/tcp/8080"
                    .parse::<Multiaddr>()
                    .unwrap()
            ),
            Ok(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(
                    65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535,
                )),
                8080,
            ))
        );
    }

    #[test]
    fn communicating_between_dialer_and_listener() {
        use std::io::Write;

        std::thread::spawn(move || {
            let addr = "/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap();
            let tcp = TcpConfig::new();
            let mut rt = Runtime::new().unwrap();
            let handle = rt.handle();
            let listener = tcp.listen_on(addr).unwrap()
                .filter_map(ListenerEvent::into_upgrade)
                .for_each(|(sock, _)| {
                    sock.and_then(|sock| {
                        // Define what to do with the socket that just connected to us
                        // Which in this case is read 3 bytes
                        let handle_conn = tokio_io::io::read_exact(sock, [0; 3])
                            .map(|(_, buf)| assert_eq!(buf, [1, 2, 3]))
                            .map_err(|err| panic!("IO error {:?}", err));

                        // Spawn the future as a concurrent task
                        handle.spawn(handle_conn).unwrap();

                        Ok(())
                    })
                });

            rt.block_on(listener).unwrap();
            rt.run().unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        let addr = "/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap();
        let tcp = TcpConfig::new();
        // Obtain a future socket through dialing
        let socket = tcp.dial(addr.clone()).unwrap();
        // Define what to do with the socket once it's obtained
        let action = socket.then(|sock| -> Result<(), ()> {
            sock.unwrap().write(&[0x1, 0x2, 0x3]).unwrap();
            Ok(())
        });
        // Execute the future in our event loop
        let mut rt = Runtime::new().unwrap();
        let _ = rt.block_on(action).unwrap();
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv4() {
        let tcp = TcpConfig::new();

        let addr = "/ip4/127.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let new_addr = tcp.listen_on(addr).unwrap().wait()
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv6() {
        let tcp = TcpConfig::new();

        let addr: Multiaddr = "/ip6/::1/tcp/0".parse().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let new_addr = tcp.listen_on(addr).unwrap().wait()
            .next()
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[test]
    fn larger_addr_denied() {
        let tcp = TcpConfig::new();

        let addr = "/ip4/127.0.0.1/tcp/12345/tcp/12345"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(tcp.listen_on(addr).is_err());
    }

    #[test]
    fn nat_traversal() {
        let tcp = TcpConfig::new();

        let server = "/ip4/127.0.0.1/tcp/10000".parse::<Multiaddr>().unwrap();
        let observed = "/ip4/80.81.82.83/tcp/25000".parse::<Multiaddr>().unwrap();

        let out = tcp.nat_traversal(&server, &observed);
        assert_eq!(
            out.unwrap(),
            "/ip4/80.81.82.83/tcp/10000".parse::<Multiaddr>().unwrap()
        );
    }

    #[test]
    fn nat_traversal_ipv6_to_ipv4() {
        let tcp = TcpConfig::new();

        let server = "/ip6/::1/tcp/10000".parse::<Multiaddr>().unwrap();
        let observed = "/ip4/80.81.82.83/tcp/25000".parse::<Multiaddr>().unwrap();

        let out = tcp.nat_traversal(&server, &observed);
        assert_eq!(
            out.unwrap(),
            "/ip4/80.81.82.83/tcp/10000".parse::<Multiaddr>().unwrap()
        );
    }

    #[test]
    fn nat_traversal_ipv4_to_ipv6() {
        let tcp = TcpConfig::new();

        let server = "/ip4/127.0.0.1/tcp/10000".parse::<Multiaddr>().unwrap();
        let observed = "/ip6/2001:db8::1/tcp/25000".parse::<Multiaddr>().unwrap();

        let out = tcp.nat_traversal(&server, &observed);
        assert_eq!(
            out.unwrap(),
            "/ip6/2001:db8::1/tcp/10000".parse::<Multiaddr>().unwrap()
        );
    }
}
