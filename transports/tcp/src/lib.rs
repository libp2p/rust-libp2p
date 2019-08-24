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
use get_if_addrs::{IfAddr, get_if_addrs};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use libp2p_core::{
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{ListenerEvent, TransportError}
};
use log::{debug, trace};
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    iter::{self, FromIterator},
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
    vec::IntoIter
};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_timer::Delay;
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
                debug!("Listening on {:?}", addrs.iter().map(|(_, _, ma)| ma).collect::<Vec<_>>());
                Addresses::Many(addrs)
            } else {
                let ma = ip_to_multiaddr(local_addr.ip(), port);
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
                let events = aa.iter()
                    .map(|(_, _, ma)| ma)
                    .cloned()
                    .map(ListenerEvent::NewAddress)
                    .collect::<Vec<_>>();
                Either::B(stream::iter_ok(events))
            }
        };

        let stream = TcpListenStream {
            inner: Listener::new(listener.incoming(), self.sleep_on_error),
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
fn ip_to_multiaddr(ip: IpAddr, port: u16) -> Multiaddr {
    let proto = match ip {
        IpAddr::V4(ip) => Protocol::Ip4(ip),
        IpAddr::V6(ip) => Protocol::Ip6(ip)
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
    Many(Vec<(IpAddr, IpNet, Multiaddr)>)
}

type Buffer = VecDeque<ListenerEvent<FutureResult<TcpTransStream, io::Error>>>;

/// Incoming connection stream which pauses after errors.
#[derive(Debug)]
struct Listener<S> {
    /// The incoming connections.
    stream: S,
    /// The current pause if any.
    pause: Option<Delay>,
    /// How long to pause after an error.
    pause_duration: Duration
}

impl<S> Listener<S>
where
    S: Stream,
    S::Error: std::fmt::Display
{
    fn new(stream: S, duration: Duration) -> Self {
        Listener { stream, pause: None, pause_duration: duration }
    }
}

impl<S> Stream for Listener<S>
where
    S: Stream,
    S::Error: std::fmt::Display
{
    type Item = S::Item;
    type Error = S::Error;

    /// Polls for incoming connections, pausing if an error is encountered.
    fn poll(&mut self) -> Poll<Option<S::Item>, S::Error> {
        match self.pause.as_mut().map(|p| p.poll()) {
            Some(Ok(Async::NotReady)) => return Ok(Async::NotReady),
            Some(Ok(Async::Ready(()))) | Some(Err(_)) => { self.pause.take(); }
            None => ()
        }

        match self.stream.poll() {
            Ok(x) => Ok(x),
            Err(e) => {
                debug!("error accepting incoming connection: {}", e);
                self.pause = Some(Delay::new(Instant::now() + self.pause_duration));
                Err(e)
            }
        }
    }
}

/// Stream that listens on an TCP/IP address.
#[derive(Debug)]
pub struct TcpListenStream {
    /// Stream of incoming sockets.
    inner: Listener<Incoming>,
    /// The port which we use as our listen port in listener event addresses.
    port: u16,
    /// The set of known addresses.
    addrs: Addresses,
    /// Temporary buffer of listener events.
    pending: Buffer,
    /// Original configuration.
    config: TcpConfig
}

// If we listen on all interfaces, find out to which interface the given
// socket address belongs. In case we think the address is new, check
// all host interfaces again and report new and expired listen addresses.
fn check_for_interface_changes(
    socket_addr: &SocketAddr,
    listen_port: u16,
    listen_addrs: &mut Vec<(IpAddr, IpNet, Multiaddr)>,
    pending: &mut Buffer
) -> Result<(), io::Error> {
    // Check for exact match:
    if listen_addrs.iter().find(|(ip, ..)| ip == &socket_addr.ip()).is_some() {
        return Ok(())
    }

    // No exact match => check netmask
    if listen_addrs.iter().find(|(_, net, _)| net.contains(&socket_addr.ip())).is_some() {
        return Ok(())
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
            pending.push_back(ListenerEvent::AddressExpired(ma.clone()));
        }
    }

    // Check for new addresses.
    for (ip, _, ma) in listen_addrs.iter() {
        if old_listen_addrs.iter().find(|(i, ..)| i == ip).is_none() {
            debug!("New listen address: {}", ma);
            pending.push_back(ListenerEvent::NewAddress(ma.clone()));
        }
    }

    // We should now be able to find the local address, if not something
    // is seriously wrong and we report an error.
    if listen_addrs.iter()
        .find(|(ip, net, _)| ip == &socket_addr.ip() || net.contains(&socket_addr.ip()))
        .is_none()
    {
        let msg = format!("{} does not match any listen address", socket_addr.ip());
        return Err(io::Error::new(io::ErrorKind::Other, msg))
    }

    Ok(())
}

impl Stream for TcpListenStream {
    type Item = ListenerEvent<FutureResult<TcpTransStream, io::Error>>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, io::Error> {
        loop {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Async::Ready(Some(event)))
            }

            let sock = match self.inner.poll() {
                Ok(Async::Ready(Some(sock))) => sock,
                Ok(Async::Ready(None)) => return Ok(Async::Ready(None)),
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(e) => return Err(e)
            };

            let sock_addr = match sock.peer_addr() {
                Ok(addr) => addr,
                Err(err) => {
                    debug!("Failed to get peer address: {:?}", err);
                    continue
                }
            };

            let local_addr = match sock.local_addr() {
                Ok(sock_addr) => {
                    if let Addresses::Many(ref mut addrs) = self.addrs {
                        check_for_interface_changes(&sock_addr, self.port, addrs, &mut self.pending)?
                    }
                    ip_to_multiaddr(sock_addr.ip(), sock_addr.port())
                }
                Err(err) => {
                    debug!("Failed to get local address of incoming socket: {:?}", err);
                    continue
                }
            };

            let remote_addr = ip_to_multiaddr(sock_addr.ip(), sock_addr.port());

            match apply_config(&self.config, &sock) {
                Ok(()) => {
                    trace!("Incoming connection from {} at {}", remote_addr, local_addr);
                    self.pending.push_back(ListenerEvent::Upgrade {
                        upgrade: future::ok(TcpTransStream { inner: sock }),
                        local_addr,
                        remote_addr
                    })
                }
                Err(err) => {
                    debug!("Error upgrading incoming connection from {}: {:?}", remote_addr, err);
                    self.pending.push_back(ListenerEvent::Upgrade {
                        upgrade: future::err(err),
                        local_addr,
                        remote_addr
                    })
                }
            }
        }
    }
}

/// Wraps around a `TcpStream` and adds logging for important events.
#[derive(Debug)]
pub struct TcpTransStream {
    inner: TcpStream,
}

impl Read for TcpTransStream {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, io::Error> {
        self.inner.read(buf)
    }
}

impl AsyncRead for TcpTransStream {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.inner.prepare_uninitialized_buffer(buf)
    }

    fn read_buf<B: bytes::BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        self.inner.read_buf(buf)
    }
}

impl Write for TcpTransStream {
    fn write(&mut self, buf: &[u8]) -> Result<usize, io::Error> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        self.inner.flush()
    }
}

impl AsyncWrite for TcpTransStream {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        AsyncWrite::shutdown(&mut self.inner)
    }
}

impl Drop for TcpTransStream {
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
    use futures::{prelude::*, future::{self, Loop}, stream};
    use libp2p_core::{Transport, multiaddr::{Multiaddr, Protocol}, transport::ListenerEvent};
    use std::{net::{IpAddr, Ipv4Addr, SocketAddr}, time::Duration};
    use super::{multiaddr_to_socketaddr, TcpConfig, Listener};
    use tokio::runtime::current_thread::{self, Runtime};
    use tokio_io;

    #[test]
    fn pause_on_error() {
        // We create a stream of values and errors and continue polling even after errors
        // have been encountered. We count the number of items (including errors) and assert
        // that no item has been missed.
        let rs = stream::iter_result(vec![Ok(1), Err(1), Ok(1), Err(1)]);
        let ls = Listener::new(rs, Duration::from_secs(1));
        let sum = future::loop_fn((0, ls), |(acc, ls)| {
            ls.into_future().then(move |item| {
                match item {
                    Ok((None, _)) => Ok::<_, std::convert::Infallible>(Loop::Break(acc)),
                    Ok((Some(n), rest)) => Ok(Loop::Continue((acc + n, rest))),
                    Err((n, rest)) => Ok(Loop::Continue((acc + n, rest)))
                }
            })
        });
        assert_eq!(4, current_thread::block_on_all(sum).unwrap())
    }

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
}
