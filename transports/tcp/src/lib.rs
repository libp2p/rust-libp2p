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
//! # Usage
//!
//! This crate provides two structs, `TcpConfig` and `TokioTcpConfig`, depending on which
//! features are enabled.
//!
//! Both the `TcpConfig` and `TokioTcpConfig` structs implement the `Transport` trait of the
//! `core` library. See the documentation of `core` and of libp2p in general to learn how to
//! use the `Transport` trait.

use async_io::Async;
use futures::{future::{self, Ready}, prelude::*};
use futures_timer::Delay;
use get_if_addrs::{IfAddr, get_if_addrs};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use libp2p_core::{
    Dialer,
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{ListenerEvent, TransportError}
};
use log::{debug, trace};
use socket2::{Socket, Domain, Type};
use std::{
    collections::VecDeque,
    io,
    iter::{self, FromIterator},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    pin::Pin,
    task::{Context, Poll},
    time::Duration
};

/// Represents the configuration for a TCP/IP transport capability for libp2p.
///
/// The TCP sockets created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through the tokio reactor.
#[derive(Debug, Clone, Default)]
pub struct TcpConfig {
    /// How long a listener should sleep after receiving an error, before trying again.
    sleep_on_error: Duration,
    /// TTL to set for opened sockets, or `None` to keep default.
    ttl: Option<u32>,
    /// `TCP_NODELAY` to set for opened sockets, or `None` to keep default.
    nodelay: Option<bool>,
}

impl TcpConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> TcpConfig {
        TcpConfig {
            sleep_on_error: Duration::from_millis(100),
            ttl: None,
            nodelay: None,
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
}

impl TcpConfig {
    fn do_listen(self, socket_addr: SocketAddr) -> Result<(TcpListenStream, TcpDialer), io::Error> {
        let domain = if socket_addr.is_ipv4() {
            Domain::ipv4()
        } else {
            Domain::ipv6()
        };
        let socket = Socket::new(domain, Type::stream(), Some(socket2::Protocol::tcp()))?;
        if socket_addr.is_ipv6() {
            socket.set_only_v6(true)?;
        }
        if let Some(ttl) = self.ttl {
            socket.set_ttl(ttl)?;
        }
        if let Some(nodelay) = self.nodelay {
            socket.set_nodelay(nodelay)?;
        }
        socket.set_reuse_address(true)?;
        if cfg!(unix) {
            socket.set_reuse_port(true)?;
        }
        socket.bind(&socket_addr.into())?;
        socket.listen(1024)?; // we may want to make this configurable

        let listener = Async::new(socket.into_tcp_listener())?;
        let local_addr = listener.get_ref().local_addr()?;
        let port = local_addr.port();

        // Determine all our listen addresses which is either a single local IP address
        // or (if a wildcard IP address was used) the addresses of all our interfaces,
        // as reported by `get_if_addrs`.
        let addrs =
            if socket_addr.ip().is_unspecified() {
                let addrs = host_addresses(port)?;
                debug!("Listening on {:?}", addrs.iter().map(|(_, _, ma)| ma).collect::<Vec<_>>());
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
            Addresses::Many(ref aa) => {
                aa.iter()
                    .map(|(_, _, ma)| ma)
                    .cloned()
                    .map(ListenerEvent::NewAddress)
                    .map(Result::Ok)
                    .collect::<VecDeque<_>>()
            }
        };

        let dialer = TcpDialer {
            ttl: self.ttl,
            nodelay: self.nodelay,
            local_socket_addr: Some(local_addr),
        };
        let listen_stream = TcpListenStream {
            stream: listener,
            pause: None,
            pause_duration: self.sleep_on_error,
            port,
            addrs,
            pending,
        };
        Ok((listen_stream, dialer))
    }
}

/// TCP/IP dialer.
#[derive(Debug, Clone)]
pub struct TcpDialer {
    /// TTL to set for opened sockets, or `None` to keep default.
    ttl: Option<u32>,
    /// `TCP_NODELAY` to set for opened sockets, or `None` to keep default.
    nodelay: Option<bool>,
    /// Local socket address.
    local_socket_addr: Option<SocketAddr>,
}

impl TcpDialer {
    async fn do_dial(
        self,
        socket_addr: SocketAddr,
    ) -> Result<Async<TcpStream>, io::Error> {
        let domain = if socket_addr.is_ipv4() {
            Domain::ipv4()
        } else {
            Domain::ipv6()
        };
        let socket = Socket::new(domain, Type::stream(), Some(socket2::Protocol::tcp()))?;
        if socket_addr.is_ipv6() {
            socket.set_only_v6(true)?;
        }
        if let Some(ttl) = self.ttl {
            socket.set_ttl(ttl)?;
        }
        if let Some(nodelay) = self.nodelay {
            socket.set_nodelay(nodelay)?;
        }
        socket.set_reuse_address(true)?;
        if cfg!(unix) {
            socket.set_reuse_port(true)?;
        }
        if let Some(local_socket_addr) = self.local_socket_addr {
            socket.bind(&local_socket_addr.into())?;
        }
        socket.set_nonblocking(true)?;
        match socket.connect(&socket_addr.into()) {
            Ok(()) => {}
            Err(err) if err.raw_os_error() == Some(libc::EINPROGRESS) => {}
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
            Err(err) => return Err(err),
        };
        let stream = Async::new(socket.into_tcp_stream())?;
        stream.writable().await?;
        Ok(stream)
    }
}

impl Dialer for TcpDialer {
    type Output = Async<TcpStream>;
    type Error = io::Error;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr =
            if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
                if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                    debug!("Instantly refusing dialing {}, as it is invalid", addr);
                    return Err(TransportError::Other(io::ErrorKind::ConnectionRefused.into()));
                }
                if let Some(local_socket_addr) = self.local_socket_addr.as_ref() {
                    if local_socket_addr.is_ipv4() != socket_addr.is_ipv4() {
                        return Err(TransportError::MultiaddrNotSupported(addr));
                    }
                }
                socket_addr
            } else {
                return Err(TransportError::MultiaddrNotSupported(addr));
            };

        debug!("Dialing {}", addr);

        Ok(Box::pin(self.do_dial(socket_addr)))
    }

    fn requires_address_translation(&self) -> bool {
        self.local_socket_addr.is_none()
    }
}

impl Transport for TcpConfig {
    type Output = Async<TcpStream>;
    type Error = io::Error;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type Dialer = TcpDialer;
    type Listener = TcpListenStream;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;

    fn dialer(&self) -> Self::Dialer {
        TcpDialer {
            ttl: self.ttl,
            nodelay: self.nodelay,
            local_socket_addr: None,
        }
    }

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Self::Dialer), TransportError<Self::Error>> {
        let socket_addr =
            if let Ok(sa) = multiaddr_to_socketaddr(&addr) {
                sa
            } else {
                return Err(TransportError::MultiaddrNotSupported(addr))
            };

        self.do_listen(socket_addr)
            .map_err(|e| TransportError::Other(e))
    }
}

/// Stream that listens on an TCP/IP address.
pub struct TcpListenStream {
    /// The incoming connections.
    stream: Async<TcpListener>,
    /// The current pause if any.
    pause: Option<Delay>,
    /// How long to pause after an error.
    pause_duration: Duration,
    /// The port which we use as our listen port in listener event addresses.
    port: u16,
    /// The set of known addresses.
    addrs: Addresses,
    /// Temporary buffer of listener events.
    pending: Buffer<Async<TcpStream>>,
}

impl Stream for TcpListenStream {
    type Item =  Result<ListenerEvent<Ready<Result<Async<TcpStream>, io::Error>>, io::Error>, io::Error>;

    /// Takes ownership of the listener, and returns the next incoming event and the listener.
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let me = Pin::into_inner(self);
        loop {
            if let Some(event) = me.pending.pop_front() {
                return Poll::Ready(Some(event));
            }

            if let Some(mut pause) = me.pause.take() {
                match Pin::new(&mut pause).poll(cx) {
                    Poll::Ready(()) => {}
                    Poll::Pending => {
                        me.pause = Some(pause);
                        return Poll::Pending;
                    }
                }
            }

            let (stream, sock_addr) = loop {
                match me.stream.accept().now_or_never() {
                    Some(Ok(res)) => break res,
                    Some(Err(e)) => {
                        debug!("error accepting incoming connection: {}", e);
                        me.pause = Some(Delay::new(me.pause_duration));
                        return Poll::Ready(Some(Ok(ListenerEvent::Error(e))));
                    }
                    None => {
                        match me.stream.poll_readable(cx) {
                            Poll::Ready(Ok(())) => continue,
                            Poll::Pending => return Poll::Pending,
                            Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
                        }
                    }
                }
            };

            let local_addr = match stream.get_ref().local_addr() {
                Ok(sock_addr) => {
                    if let Addresses::Many(ref mut addrs) = me.addrs {
                        if let Err(err) = check_for_interface_changes(&sock_addr, me.port, addrs, &mut me.pending) {
                            return Poll::Ready(Some(Ok(ListenerEvent::Error(err))));
                        }
                    }
                    ip_to_multiaddr(sock_addr.ip(), sock_addr.port())
                }
                Err(err) => {
                    debug!("Failed to get local address of incoming socket: {:?}", err);
                    continue
                }
            };

            let remote_addr = ip_to_multiaddr(sock_addr.ip(), sock_addr.port());

            trace!("Incoming connection from {} at {}", remote_addr, local_addr);
            me.pending.push_back(Ok(ListenerEvent::Upgrade {
                upgrade: future::ok(stream),
                local_addr,
                remote_addr
            }));
        }
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

/// Listen address information.
#[derive(Debug)]
enum Addresses {
    /// A specific address is used to listen.
    One(Multiaddr),
    /// A set of addresses is used to listen.
    Many(Vec<(IpAddr, IpNet, Multiaddr)>)
}

type Buffer<T> = VecDeque<Result<ListenerEvent<Ready<Result<T, io::Error>>, io::Error>, io::Error>>;

// If we listen on all interfaces, find out to which interface the given
// socket address belongs. In case we think the address is new, check
// all host interfaces again and report new and expired listen addresses.
fn check_for_interface_changes<T>(
    socket_addr: &SocketAddr,
    listen_port: u16,
    listen_addrs: &mut Vec<(IpAddr, IpNet, Multiaddr)>,
    pending: &mut Buffer<T>
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
    if listen_addrs.iter()
        .find(|(ip, net, _)| ip == &socket_addr.ip() || net.contains(&socket_addr.ip()))
        .is_none()
    {
        let msg = format!("{} does not match any listen address", socket_addr.ip());
        return Err(io::Error::new(io::ErrorKind::Other, msg))
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use futures::prelude::*;
    use libp2p_core::{Dialer, Transport, multiaddr::{Multiaddr, Protocol}, transport::ListenerEvent};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use super::{multiaddr_to_socketaddr, TcpConfig};

    #[test]
    fn wildcard_expansion() {
        fn test(addr: Multiaddr) {
            let mut listener = TcpConfig::new().listen_on(addr).expect("listener").0;

            // Get the first address.
            let addr = futures::executor::block_on_stream(listener.by_ref())
                .next()
                .expect("some event")
                .expect("no error")
                .into_new_address()
                .expect("listen address");

            // Process all initial `NewAddress` events and make sure they
            // do not contain wildcard address or port.
            let server = listener
                .take_while(|event| match event.as_ref().unwrap() {
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
                        futures::future::ready(true)
                    }
                    _ => futures::future::ready(false)
                })
                .for_each(|_| futures::future::ready(()));

            let client = TcpConfig::new().dialer().dial(addr).expect("dialer");
            async_std::task::block_on(futures::future::join(server, client)).1.unwrap();
        }

        test("/ip4/0.0.0.0/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
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
        env_logger::try_init().ok();
        fn test(addr: Multiaddr) {
            let (ready_tx, ready_rx) = futures::channel::oneshot::channel();
            let mut ready_tx = Some(ready_tx);

            async_std::task::spawn(async move {
                let tcp = TcpConfig::new();
                let mut listener = tcp.listen_on(addr).unwrap().0;

                loop {
                    match listener.next().await.unwrap().unwrap() {
                        ListenerEvent::NewAddress(listen_addr) => {
                            ready_tx.take().unwrap().send(listen_addr).unwrap();
                        },
                        ListenerEvent::Upgrade { upgrade, .. } => {
                            let mut upgrade = upgrade.await.unwrap();
                            let mut buf = [0u8; 3];
                            upgrade.read_exact(&mut buf).await.unwrap();
                            assert_eq!(buf, [1, 2, 3]);
                            upgrade.write_all(&[4, 5, 6]).await.unwrap();
                        },
                        _ => unreachable!()
                    }
                }
            });

            async_std::task::block_on(async move {
                let addr = ready_rx.await.unwrap();
                let tcp = TcpConfig::new();

                // Obtain a future socket through dialing
                let mut socket = tcp.dialer().dial(addr.clone()).unwrap().await.unwrap();
                socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();

                let mut buf = [0u8; 3];
                socket.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [4, 5, 6]);
            });
        }

        test("/ip4/127.0.0.1/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
    }

    #[test]
    fn port_reuse() {
        env_logger::try_init().ok();
        fn test(addr: Multiaddr) {
            let (ready_tx, ready_rx) = futures::channel::oneshot::channel();
            let mut ready_tx = Some(ready_tx);
            let addr2 = addr.clone();

            async_std::task::spawn(async move {
                let tcp = TcpConfig::new();
                let (mut listener, _dialer) = tcp.listen_on(addr).unwrap();

                loop {
                    match listener.next().await.unwrap().unwrap() {
                        ListenerEvent::NewAddress(listen_addr) => {
                            ready_tx.take().unwrap().send(listen_addr).unwrap();
                        },
                        ListenerEvent::Upgrade { upgrade, .. } => {
                            let mut upgrade = upgrade.await.unwrap();
                            let mut buf = [0u8; 3];
                            upgrade.read_exact(&mut buf).await.unwrap();
                            assert_eq!(buf, [1, 2, 3]);
                            upgrade.write_all(&[4, 5, 6]).await.unwrap();
                        },
                        _ => unreachable!()
                    }
                }
            });

            async_std::task::block_on(async move {
                let dest_addr = ready_rx.await.unwrap();
                let tcp = TcpConfig::new();
                let (_listener, dialer) = tcp.listen_on(addr2).unwrap();

                // Obtain a future socket through dialing
                let mut socket = dialer.dial(dest_addr).unwrap().await.unwrap();
                socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();

                let mut buf = [0u8; 3];
                socket.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [4, 5, 6]);
            });
        }

        test("/ip4/127.0.0.1/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv4() {
        let tcp = TcpConfig::new();

        let addr = "/ip4/127.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let new_addr = futures::executor::block_on_stream(tcp.listen_on(addr).unwrap().0)
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

        let new_addr = futures::executor::block_on_stream(tcp.listen_on(addr).unwrap().0)
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
