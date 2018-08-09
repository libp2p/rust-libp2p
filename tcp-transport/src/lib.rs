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

// TODO: use this once stable ; for now we just copy-paste the content of the README.md
//#![doc(include = "../README.md")]

//! Implementation of the libp2p `Transport` trait for TCP/IP.
//!
//! Uses [the *tokio* library](https://tokio.rs).
//!
//! # Usage
//!
//! Example:
//!
//! ```
//! extern crate libp2p_tcp_transport;
//! use libp2p_tcp_transport::TcpConfig;
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

#[cfg(test)]
extern crate tokio_current_thread;

use futures::{future, future::FutureResult, prelude::*, Async, Poll};
use multiaddr::{AddrComponent, Multiaddr, ToMultiaddr};
use std::fmt;
use std::io::{Error as IoError, Read, Write};
use std::iter;
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
    sleep_on_error: Duration,
}

impl TcpConfig {
    /// Creates a new configuration object for TCP/IP.
    #[inline]
    pub fn new() -> TcpConfig {
        TcpConfig {
            sleep_on_error: Duration::from_millis(100),
        }
    }
}

impl Transport for TcpConfig {
    type Output = TcpTransStream;
    type Listener = TcpListenStream;
    type ListenerUpgrade = FutureResult<(Self::Output, Self::MultiaddrFuture), IoError>;
    type MultiaddrFuture = FutureResult<Multiaddr, IoError>;
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
            Ok((TcpListenStream { inner }, new_addr))
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
                    addr: Some(addr),
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
        // Check that `server` only has two components and retreive them.
        let mut server_protocols_iter = server.iter();
        let server_proto1 = server_protocols_iter.next()?;
        let server_proto2 = server_protocols_iter.next()?;
        if server_protocols_iter.next().is_some() {
            return None;
        }

        // Check that `observed` only has two components and retreive them.
        let mut observed_protocols_iter = observed.iter();
        let observed_proto1 = observed_protocols_iter.next()?;
        let observed_proto2 = observed_protocols_iter.next()?;
        if observed_protocols_iter.next().is_some() {
            return None;
        }

        // Check that `server` is a valid TCP/IP address.
        match (&server_proto1, &server_proto2) {
            (&AddrComponent::IP4(_), &AddrComponent::TCP(_))
            | (&AddrComponent::IP6(_), &AddrComponent::TCP(_)) => {}
            _ => return None,
        }

        // Check that `observed` is a valid TCP/IP address.
        match (&observed_proto1, &observed_proto2) {
            (&AddrComponent::IP4(_), &AddrComponent::TCP(_))
            | (&AddrComponent::IP6(_), &AddrComponent::TCP(_)) => {}
            _ => return None,
        }

        let result = iter::once(observed_proto1.clone())
            .chain(iter::once(server_proto2.clone()))
            .collect();
        Some(result)
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
        (AddrComponent::IP4(ip), AddrComponent::TCP(port)) => Ok(SocketAddr::new(ip.into(), port)),
        (AddrComponent::IP6(ip), AddrComponent::TCP(port)) => Ok(SocketAddr::new(ip.into(), port)),
        _ => Err(()),
    }
}

/// Future that dials a TCP/IP address.
#[derive(Debug)]
pub struct TcpDialFut {
    inner: ConnectFuture,
    /// Address we're dialing. Extracted when the `Future` finishes.
    addr: Option<Multiaddr>,
}

impl Future for TcpDialFut {
    type Item = (TcpTransStream, FutureResult<Multiaddr, IoError>);
    type Error = IoError;

    fn poll(&mut self) -> Poll<(TcpTransStream, FutureResult<Multiaddr, IoError>), IoError> {
        match self.inner.poll() {
            Ok(Async::Ready(stream)) => {
                let addr = self
                    .addr
                    .take()
                    .expect("TcpDialFut polled again after finished");
                let out = TcpTransStream { inner: stream };
                Ok(Async::Ready((out, future::ok(addr))))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => {
                let addr = self
                    .addr
                    .as_ref()
                    .expect("TcpDialFut polled again after finished");
                debug!("Error while dialing {:?} => {:?}", addr, err);
                Err(err)
            }
        }
    }
}

/// Stream that listens on an TCP/IP address.
pub struct TcpListenStream {
    inner: Result<SleepOnError<Incoming>, Option<IoError>>,
}

impl Stream for TcpListenStream {
    type Item = FutureResult<(TcpTransStream, FutureResult<Multiaddr, IoError>), IoError>;
    type Error = IoError;

    fn poll(
        &mut self,
    ) -> Poll<
        Option<FutureResult<(TcpTransStream, FutureResult<Multiaddr, IoError>), IoError>>,
        IoError,
    > {
        let inner = match self.inner {
            Ok(ref mut inc) => inc,
            Err(ref mut err) => {
                return Err(err.take().expect("poll called again after error"));
            }
        };

        match inner.poll() {
            Ok(Async::Ready(Some(sock))) => {
                let addr = match sock.peer_addr() {
                    // TODO: remove this expect()
                    Ok(addr) => addr
                        .to_multiaddr()
                        .expect("generating a multiaddr from a socket addr never fails"),
                    Err(err) => return Ok(Async::Ready(Some(future::err(err)))),
                };

                debug!("Incoming connection from {}", addr);
                let ret = future::ok((TcpTransStream { inner: sock }, future::ok(addr)));
                Ok(Async::Ready(Some(ret)))
            }
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(()) => unreachable!("sleep_on_error never produces an error"),
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
    use super::{multiaddr_to_socketaddr, TcpConfig};
    use futures::stream::Stream;
    use futures::Future;
    use multiaddr::Multiaddr;
    use std;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use swarm::Transport;
    use tokio_current_thread;
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
            let listener = tcp.listen_on(addr).unwrap().0.for_each(|sock| {
                sock.and_then(|(sock, _)| {
                    // Define what to do with the socket that just connected to us
                    // Which in this case is read 3 bytes
                    let handle_conn = tokio_io::io::read_exact(sock, [0; 3])
                        .map(|(_, buf)| assert_eq!(buf, [1, 2, 3]))
                        .map_err(|err| panic!("IO error {:?}", err));

                    // Spawn the future as a concurrent task
                    tokio_current_thread::spawn(handle_conn);

                    Ok(())
                })
            });

            tokio_current_thread::block_on_all(listener).unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        let addr = "/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap();
        let tcp = TcpConfig::new();
        // Obtain a future socket through dialing
        let socket = tcp.dial(addr.clone()).unwrap();
        // Define what to do with the socket once it's obtained
        let action = socket.then(|sock| -> Result<(), ()> {
            sock.unwrap().0.write(&[0x1, 0x2, 0x3]).unwrap();
            Ok(())
        });
        // Execute the future in our event loop
        tokio_current_thread::block_on_all(action).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
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
