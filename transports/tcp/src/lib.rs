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

extern crate futures;
extern crate libp2p_core as swarm;
#[macro_use]
extern crate log;
extern crate multiaddr;
extern crate tk_listen;
extern crate tokio_io;
extern crate tokio_tcp;

use futures::{future, future::FutureResult, prelude::*, Async, Poll};
use multiaddr::{Protocol, Multiaddr, ToMultiaddr};
use std::fmt;
use std::io::{Error as IoError, Read, Write};
use std::net::SocketAddr;
use std::time::Duration;
use swarm::Transport;
use tk_listen::{ListenExt, SleepOnError};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_tcp::{ConnectFuture, Incoming, TcpListener, TcpStream};

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
    #[inline]
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
    #[inline]
    pub fn recv_buffer_size(mut self, value: usize) -> Self {
        self.recv_buffer_size = Some(value);
        self
    }

    /// Sets the size of the send buffer size to set for opened sockets.
    #[inline]
    pub fn send_buffer_size(mut self, value: usize) -> Self {
        self.send_buffer_size = Some(value);
        self
    }

    /// Sets the TTL to set for opened sockets.
    #[inline]
    pub fn ttl(mut self, value: u32) -> Self {
        self.ttl = Some(value);
        self
    }

    /// Sets the keep alive pinging duration to set for opened sockets.
    #[inline]
    pub fn keepalive(mut self, value: Option<Duration>) -> Self {
        self.keepalive = Some(value);
        self
    }

    /// Sets the `TCP_NODELAY` to set for opened sockets.
    #[inline]
    pub fn nodelay(mut self, value: bool) -> Self {
        self.nodelay = Some(value);
        self
    }
}

impl Transport for TcpConfig {
    type Output = TcpTransStream;
    type Listener = TcpListenStream;
    type ListenerUpgrade = FutureResult<Self::Output, IoError>;
    type Dial = TcpDialFut;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
            let listener = TcpListener::bind(&socket_addr);
            // We need to build the `Multiaddr` to return from this function. If an error happened,
            // just return the original multiaddr.
            let new_addr = match listener {
                Ok(ref l) => if let Ok(new_s_addr) = l.local_addr() {
                    new_s_addr.to_multiaddr().expect(
                        "multiaddr generated from socket addr is \
                         always valid",
                    )
                } else {
                    addr
                },
                Err(_) => addr,
            };

            debug!("Now listening on {}", new_addr);
            let sleep_on_error = self.sleep_on_error;
            let inner = listener
                .map_err(Some)
                .map(move |l| l.incoming().sleep_on_error(sleep_on_error));
            Ok((
                TcpListenStream {
                    inner,
                    config: self,
                },
                new_addr,
            ))
        } else {
            Err((self, addr))
        }
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
            // As an optimization, we check that the address is not of the form `0.0.0.0`.
            // If so, we instantly refuse dialing instead of going through the kernel.
            if socket_addr.port() != 0 && !socket_addr.ip().is_unspecified() {
                debug!("Dialing {}", addr);
                Ok(TcpDialFut {
                    inner: TcpStream::connect(&socket_addr),
                    config: self,
                })
            } else {
                debug!("Instantly refusing dialing {}, as it is invalid", addr);
                Err((self, addr))
            }
        } else {
            Err((self, addr))
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        let mut address = Multiaddr::empty();

        // Use the observed IP address.
        match server.iter().zip(observed.iter()).next() {
            Some((Protocol::Ip4(_), x@Protocol::Ip4(_))) => address.append(x),
            Some((Protocol::Ip6(_), x@Protocol::Ip6(_))) => address.append(x),
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

/// Applies the socket configuration parameters to a socket.
fn apply_config(config: &TcpConfig, socket: &TcpStream) -> Result<(), IoError> {
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
    type Error = IoError;

    fn poll(&mut self) -> Poll<TcpTransStream, IoError> {
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

/// Stream that listens on an TCP/IP address.
pub struct TcpListenStream {
    inner: Result<SleepOnError<Incoming>, Option<IoError>>,
    /// Original configuration.
    config: TcpConfig,
}

impl Stream for TcpListenStream {
    type Item = (FutureResult<TcpTransStream, IoError>, Multiaddr);
    type Error = IoError;

    fn poll(
        &mut self,
    ) -> Poll<
        Option<(FutureResult<TcpTransStream, IoError>, Multiaddr)>,
        IoError,
    > {
        let inner = match self.inner {
            Ok(ref mut inc) => inc,
            Err(ref mut err) => {
                return Err(err.take().expect("poll called again after error"));
            }
        };

        loop {
            match inner.poll() {
                Ok(Async::Ready(Some(sock))) => {
                    let addr = match sock.peer_addr() {
                        // TODO: remove this expect()
                        Ok(addr) => addr
                            .to_multiaddr()
                            .expect("generating a multiaddr from a socket addr never fails"),
                        Err(err) => {
                            // If we can't get the address of the newly-opened socket, there's
                            // nothing we can do except ignore this connection attempt.
                            error!("Ignored incoming because could't determine its \
                                    address: {:?}", err);
                            continue
                        },
                    };

                    match apply_config(&self.config, &sock) {
                        Ok(()) => (),
                        Err(err) => return Ok(Async::Ready(Some((future::err(err), addr)))),
                    };

                    debug!("Incoming connection from {}", addr);
                    let ret = future::ok(TcpTransStream { inner: sock });
                    break Ok(Async::Ready(Some((ret, addr))))
                }
                Ok(Async::Ready(None)) => break Ok(Async::Ready(None)),
                Ok(Async::NotReady) => break Ok(Async::NotReady),
                Err(()) => unreachable!("sleep_on_error never produces an error"),
            }
        }
    }
}

impl fmt::Debug for TcpListenStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        self.inner.read(buf)
    }
}

impl AsyncRead for TcpTransStream {}

impl Write for TcpTransStream {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        self.inner.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> Result<(), IoError> {
        self.inner.flush()
    }
}

impl AsyncWrite for TcpTransStream {
    #[inline]
    fn shutdown(&mut self) -> Poll<(), IoError> {
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
    extern crate tokio;
    use self::tokio::runtime::current_thread::Runtime;
    use super::{multiaddr_to_socketaddr, TcpConfig};
    use futures::stream::Stream;
    use futures::Future;
    use multiaddr::Multiaddr;
    use std;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use swarm::Transport;
    use tokio_io;

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
            let listener = tcp.listen_on(addr).unwrap().0.for_each(|(sock, _)| {
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

        let (_, new_addr) = tcp.listen_on(addr).unwrap();
        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv6() {
        let tcp = TcpConfig::new();

        let addr: Multiaddr = "/ip6/::1/tcp/0".parse().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let (_, new_addr) = tcp.listen_on(addr).unwrap();
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
}
