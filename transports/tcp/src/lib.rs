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

mod if_task;

use async_io::{Async, Timer};
use futures::{
    channel::mpsc,
    future::{self, Ready},
    prelude::*,
};
use if_task::{IfTaskEvent, IfTaskHandle};
use libp2p_core::{
    address_translation,
    multiaddr::{Multiaddr, Protocol},
    transport::{ListenerEvent, Transport, TransportError},
};
use socket2::{Domain, Socket, Type};
use std::{
    io,
    net::{SocketAddr, TcpListener, TcpStream},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

/// Represents the configuration for a TCP/IP transport capability for libp2p.
///
/// The TCP sockets created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through the tokio reactor.
#[derive(Clone, Debug)]
pub struct TcpConfig {
    /// How long a listener should sleep after receiving an error, before trying again.
    sleep_on_error: Duration,
    /// TTL to set for opened sockets, or `None` to keep default.
    ttl: Option<u32>,
    /// `TCP_NODELAY` to set for opened sockets, or `None` to keep default.
    nodelay: Option<bool>,
    /// Incoming backlog.
    backlog: u32,
    /// Port reuse.
    port_reuse: bool,
    /// The task handle.
    handle: IfTaskHandle<SocketAddr>,
}

impl TcpConfig {
    /// Creates a new configuration object for TCP/IP.
    pub async fn new() -> io::Result<Self> {
        Ok(Self {
            sleep_on_error: Duration::from_millis(100),
            ttl: None,
            nodelay: None,
            backlog: 1024,
            port_reuse: false,
            handle: IfTaskHandle::new().await?,
        })
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

    /// Sets the backlog of incoming connections size.
    pub fn backlog(mut self, backlog: u32) -> Self {
        self.backlog = backlog;
        self
    }

    /// Enables or disables port reuse for outgoing connections.
    pub fn port_reuse(mut self, port_reuse: bool) -> Self {
        self.port_reuse = port_reuse;
        self
    }

    fn create_socket(&self, socket_addr: &SocketAddr) -> io::Result<Socket> {
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
        #[cfg(unix)]
        if self.port_reuse {
            socket.set_reuse_port(true)?;
        }
        Ok(socket)
    }

    fn do_listen(self, socket_addr: SocketAddr) -> io::Result<TcpListenStream> {
        let socket = self.create_socket(&socket_addr)?;
        socket.bind(&socket_addr.into())?;
        socket.listen(self.backlog as _)?;
        let listener = Async::new(socket.into_tcp_listener())?;
        let socket_addr = listener.get_ref().local_addr()?;

        let rx = self.handle.register_listener(&socket_addr, socket_addr);
        let listen_stream = TcpListenStream {
            config: self,
            listener,
            pause: None,
            rx,
        };
        Ok(listen_stream)
    }

    async fn do_dial(self, socket_addr: SocketAddr) -> Result<Async<TcpStream>, io::Error> {
        let socket = self.create_socket(&socket_addr)?;
        if self.port_reuse {
            if let Some(socket_addr) = self.handle.port_reuse_socket(&socket_addr).await {
                socket.bind(&socket_addr.into())?;
            }
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

impl Transport for TcpConfig {
    type Output = Async<TcpStream>;
    type Error = io::Error;
    type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;
    type Listener = TcpListenStream;
    type ListenerUpgrade = Ready<Result<Self::Output, Self::Error>>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(&addr) {
            sa
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };
        log::debug!("listening on {}", socket_addr);
        self.do_listen(socket_addr)
            .map_err(|e| TransportError::Other(e))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr = if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
            if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                return Err(TransportError::MultiaddrNotSupported(addr));
            }
            socket_addr
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };
        log::debug!("dialing {}", socket_addr);
        Ok(Box::pin(self.do_dial(socket_addr)))
    }

    fn address_translation(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        if self.port_reuse {
            None
        } else {
            address_translation(server, observed)
        }
    }
}

type TcpListenerEvent = ListenerEvent<Ready<Result<Async<TcpStream>, io::Error>>, io::Error>;

/// Stream that listens on an TCP/IP address.
pub struct TcpListenStream {
    /// The tcp config.
    config: TcpConfig,
    /// The incoming connections.
    listener: Async<TcpListener>,
    /// The current pause if any.
    pause: Option<Timer>,
    /// Address changes.
    rx: mpsc::UnboundedReceiver<IfTaskEvent>,
}

impl Stream for TcpListenStream {
    type Item = Result<TcpListenerEvent, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let me = Pin::into_inner(self);
        loop {
            match Pin::new(&mut me.rx).poll_next(cx) {
                Poll::Ready(Some(IfTaskEvent::NewAddress(socket_addr))) => {
                    let addr = socketaddr_to_multiaddr(&socket_addr);
                    log::debug!("new address {}", addr);
                    return Poll::Ready(Some(Ok(ListenerEvent::NewAddress(addr))));
                }
                Poll::Ready(Some(IfTaskEvent::AddressExpired(socket_addr))) => {
                    let addr = socketaddr_to_multiaddr(&socket_addr);
                    log::debug!("address expired {}", addr);
                    return Poll::Ready(Some(Ok(ListenerEvent::AddressExpired(addr))));
                }
                Poll::Pending => {}
                Poll::Ready(None) => {}
            }

            if let Some(mut pause) = me.pause.take() {
                match Pin::new(&mut pause).poll(cx) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        me.pause = Some(pause);
                        return Poll::Pending;
                    }
                }
            }

            match me.listener.poll_readable(cx) {
                Poll::Ready(Ok(())) => {}
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(err)) => return Poll::Ready(Some(Err(err))),
            }
            let (stream, sock_addr) = match me.listener.accept().now_or_never() {
                Some(Ok(res)) => res,
                Some(Err(e)) => {
                    log::error!("error accepting incoming connection: {}", e);
                    me.pause = Some(Timer::after(me.config.sleep_on_error));
                    return Poll::Ready(Some(Ok(ListenerEvent::Error(e))));
                }
                None => unreachable!(),
            };
            let local_addr = match stream.get_ref().local_addr() {
                Ok(sock_addr) => socketaddr_to_multiaddr(&sock_addr),
                Err(err) => {
                    log::error!("Failed to get local address of incoming socket: {:?}", err);
                    continue;
                }
            };
            let remote_addr = socketaddr_to_multiaddr(&sock_addr);

            log::debug!("Incoming connection from {} at {}", remote_addr, local_addr);
            return Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
                upgrade: future::ok(stream),
                local_addr,
                remote_addr,
            })));
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
fn socketaddr_to_multiaddr(addr: &SocketAddr) -> Multiaddr {
    Multiaddr::empty()
        .with(addr.ip().into())
        .with(Protocol::Tcp(addr.port()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiaddr_to_tcp_conversion() {
        use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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
            let (mut ready_tx, mut ready_rx) = mpsc::channel(1);

            async_std::task::spawn(async move {
                let tcp = TcpConfig::new().await.unwrap();
                let mut listener = tcp.listen_on(addr).unwrap();

                loop {
                    match listener.next().await.unwrap().unwrap() {
                        ListenerEvent::NewAddress(listen_addr) => {
                            ready_tx.send(listen_addr).await.unwrap();
                        }
                        ListenerEvent::Upgrade { upgrade, .. } => {
                            let mut upgrade = upgrade.await.unwrap();
                            let mut buf = [0u8; 3];
                            upgrade.read_exact(&mut buf).await.unwrap();
                            assert_eq!(buf, [1, 2, 3]);
                            upgrade.write_all(&[4, 5, 6]).await.unwrap();
                        }
                        _ => unreachable!(),
                    }
                }
            });

            async_std::task::block_on(async move {
                let addr = ready_rx.next().await.unwrap();
                let tcp = TcpConfig::new().await.unwrap();

                // Obtain a future socket through dialing
                let mut socket = tcp.dial(addr.clone()).unwrap().await.unwrap();
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
    fn wildcard_expansion() {
        env_logger::try_init().ok();

        fn test(addr: Multiaddr) {
            let (mut ready_tx, mut ready_rx) = mpsc::channel(1);

            async_std::task::spawn(async move {
                let tcp = TcpConfig::new().await.unwrap();
                let mut listener = tcp.listen_on(addr).unwrap();

                loop {
                    match listener.next().await.unwrap().unwrap() {
                        ListenerEvent::NewAddress(a) => {
                            let mut iter = a.iter();
                            match iter.next().expect("ip address") {
                                Protocol::Ip4(ip) => assert!(!ip.is_unspecified()),
                                Protocol::Ip6(ip) => assert!(!ip.is_unspecified()),
                                other => panic!("Unexpected protocol: {}", other),
                            }
                            if let Protocol::Tcp(port) = iter.next().expect("port") {
                                assert_ne!(0, port)
                            } else {
                                panic!("No TCP port in address: {}", a)
                            }
                            ready_tx.send(a).await.ok();
                        }
                        _ => {}
                    }
                }
            });

            async_std::task::block_on(async move {
                let dest_addr = ready_rx.next().await.unwrap();
                let tcp = TcpConfig::new().await.unwrap();
                tcp.dial(dest_addr).unwrap().await.unwrap();
            });
        }

        test("/ip4/0.0.0.0/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
    }

    #[test]
    fn port_reuse() {
        env_logger::try_init().ok();

        fn test(addr: Multiaddr) {
            let (mut ready_tx, mut ready_rx) = mpsc::channel(1);
            let addr2 = addr.clone();

            async_std::task::spawn(async move {
                let tcp = TcpConfig::new().await.unwrap();
                let mut listener = tcp.listen_on(addr).unwrap();

                loop {
                    match listener.next().await.unwrap().unwrap() {
                        ListenerEvent::NewAddress(listen_addr) => {
                            ready_tx.send(listen_addr).await.ok();
                        }
                        ListenerEvent::Upgrade { upgrade, .. } => {
                            let mut upgrade = upgrade.await.unwrap();
                            let mut buf = [0u8; 3];
                            upgrade.read_exact(&mut buf).await.unwrap();
                            assert_eq!(buf, [1, 2, 3]);
                            upgrade.write_all(&[4, 5, 6]).await.unwrap();
                        }
                        _ => unreachable!(),
                    }
                }
            });

            async_std::task::block_on(async move {
                let dest_addr = ready_rx.next().await.unwrap();
                let tcp = TcpConfig::new().await.unwrap().port_reuse(true);
                let _listener = tcp.clone().listen_on(addr2).unwrap();

                // Obtain a future socket through dialing
                let mut socket = tcp.dial(dest_addr).unwrap().await.unwrap();
                socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();

                let mut buf = [0u8; 3];
                socket.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [4, 5, 6]);
            });
        }

        test("/ip4/127.0.0.1/tcp/0".parse().unwrap());
        test("/ip6/::1/tcp/0".parse().unwrap());
    }

    #[async_std::test]
    async fn replace_port_0_in_returned_multiaddr_ipv4() {
        env_logger::try_init().ok();

        let tcp = TcpConfig::new().await.unwrap();

        let addr = "/ip4/127.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let new_addr = tcp
            .listen_on(addr)
            .unwrap()
            .next()
            .await
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[async_std::test]
    async fn replace_port_0_in_returned_multiaddr_ipv6() {
        env_logger::try_init().ok();

        let tcp = TcpConfig::new().await.unwrap();

        let addr: Multiaddr = "/ip6/::1/tcp/0".parse().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let new_addr = tcp
            .listen_on(addr)
            .unwrap()
            .next()
            .await
            .expect("some event")
            .expect("no error")
            .into_new_address()
            .expect("listen address");

        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[async_std::test]
    async fn larger_addr_denied() {
        env_logger::try_init().ok();

        let tcp = TcpConfig::new().await.unwrap();

        let addr = "/ip4/127.0.0.1/tcp/12345/tcp/12345"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(tcp.listen_on(addr).is_err());
    }
}
