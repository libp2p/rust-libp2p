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
//! Create [a tokio `Core`](https://docs.rs/tokio-core/0.1/tokio_core/reactor/struct.Core.html),
//! then grab a handle by calling the `handle()` method on it, then create a `TcpConfig` and pass
//! the handle.
//!
//! Example:
//!
//! ```
//! extern crate libp2p_tcp_transport;
//! extern crate tokio_core;
//!
//! use libp2p_tcp_transport::TcpConfig;
//! use tokio_core::reactor::Core;
//!
//! # fn main() {
//! let mut core = Core::new().unwrap();
//! let tcp = TcpConfig::new(core.handle());
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
extern crate tokio_core;
extern crate tokio_io;

use futures::future::{self, Future, FutureResult};
use futures::stream::Stream;
use multiaddr::{AddrComponent, Multiaddr, ToMultiaddr};
use std::io::Error as IoError;
use std::iter;
use std::net::SocketAddr;
use swarm::Transport;
use tokio_core::net::{TcpListener, TcpStream};
use tokio_core::reactor::Handle;

/// Represents the configuration for a TCP/IP transport capability for libp2p.
///
/// Each connection created by this config is tied to a tokio reactor. The TCP sockets created by
/// libp2p will need to be progressed by running the futures and streams obtained by libp2p
/// through the tokio reactor.
#[derive(Debug, Clone)]
pub struct TcpConfig {
    event_loop: Handle,
}

impl TcpConfig {
    /// Creates a new configuration object for TCP/IP. The `Handle` is a tokio reactor the
    /// connections will be created with.
    #[inline]
    pub fn new(handle: Handle) -> TcpConfig {
        TcpConfig { event_loop: handle }
    }
}

impl Transport for TcpConfig {
    type Output = TcpStream;
    type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
    type ListenerUpgrade = FutureResult<(Self::Output, Self::MultiaddrFuture), IoError>;
    type MultiaddrFuture = FutureResult<Multiaddr, IoError>;
    type Dial = Box<Future<Item = (TcpStream, Self::MultiaddrFuture), Error = IoError>>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
            let listener = TcpListener::bind(&socket_addr, &self.event_loop);
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

            let future = future::result(listener)
                .map(|listener| {
                    // Pull out a stream of sockets for incoming connections
                    listener.incoming().map(|(sock, addr)| {
                        let addr = addr.to_multiaddr()
                            .expect("generating a multiaddr from a socket addr never fails");
                        debug!("Incoming connection from {}", addr);
                        future::ok((sock, future::ok(addr)))
                    })
                })
                .flatten_stream();
            Ok((Box::new(future), new_addr))
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
                let fut = TcpStream::connect(&socket_addr, &self.event_loop).map(|t| (t, future::ok(addr)));
                Ok(Box::new(fut) as Box<_>)
            } else {
                debug!("Instantly refusing dialing {}, as it is invalid", addr);
                Err((self, addr))
            }
        } else {
            Err((self, addr))
        }
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        let server_protocols: Vec<_> = server.iter().collect();
        let observed_protocols: Vec<_> = observed.iter().collect();

        if server_protocols.len() != 2 || observed_protocols.len() != 2 {
            return None;
        }

        // Check that `server` is a valid TCP/IP address.
        match (&server_protocols[0], &server_protocols[1]) {
            (&AddrComponent::IP4(_), &AddrComponent::TCP(_))
            | (&AddrComponent::IP6(_), &AddrComponent::TCP(_)) => {}
            _ => return None,
        }

        // Check that `observed` is a valid TCP/IP address.
        match (&observed_protocols[0], &observed_protocols[1]) {
            (&AddrComponent::IP4(_), &AddrComponent::TCP(_))
            | (&AddrComponent::IP6(_), &AddrComponent::TCP(_)) => {}
            _ => return None,
        }

        let result = iter::once(observed_protocols[0].clone())
            .chain(iter::once(server_protocols[1].clone()))
            .collect();

        Some(result)
    }
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Result<SocketAddr, ()> {
    let protocols: Vec<_> = addr.iter().collect();

    if protocols.len() != 2 {
        return Err(());
    }

    match (&protocols[0], &protocols[1]) {
        (&AddrComponent::IP4(ref ip), &AddrComponent::TCP(port)) => {
            Ok(SocketAddr::new(ip.clone().into(), port))
        }
        (&AddrComponent::IP6(ref ip), &AddrComponent::TCP(port)) => {
            Ok(SocketAddr::new(ip.clone().into(), port))
        }
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{multiaddr_to_socketaddr, TcpConfig};
    use futures::Future;
    use futures::stream::Stream;
    use multiaddr::Multiaddr;
    use std;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use swarm::Transport;
    use tokio_core::reactor::Core;
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
            multiaddr_to_socketaddr(&"/ip4/255.255.255.255/tcp/8080"
                .parse::<Multiaddr>()
                .unwrap()),
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
            multiaddr_to_socketaddr(&"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/tcp/8080"
                .parse::<Multiaddr>()
                .unwrap()),
            Ok(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::new(
                    65535,
                    65535,
                    65535,
                    65535,
                    65535,
                    65535,
                    65535,
                    65535,
                )),
                8080,
            ))
        );
    }

    #[test]
    fn communicating_between_dialer_and_listener() {
        use std::io::Write;

        std::thread::spawn(move || {
            let mut core = Core::new().unwrap();
            let addr = "/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap();
            let tcp = TcpConfig::new(core.handle());
            let handle = core.handle();
            let listener = tcp.listen_on(addr).unwrap().0.for_each(|sock| {
                sock.and_then(|(sock, _)| {
                    // Define what to do with the socket that just connected to us
                    // Which in this case is read 3 bytes
                    let handle_conn = tokio_io::io::read_exact(sock, [0; 3])
                        .map(|(_, buf)| assert_eq!(buf, [1, 2, 3]))
                        .map_err(|err| panic!("IO error {:?}", err));

                    // Spawn the future as a concurrent task
                    handle.spawn(handle_conn);

                    Ok(())
                })
            });

            core.run(listener).unwrap();
        });
        std::thread::sleep(std::time::Duration::from_millis(100));
        let addr = "/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap();
        let mut core = Core::new().unwrap();
        let tcp = TcpConfig::new(core.handle());
        // Obtain a future socket through dialing
        let socket = tcp.dial(addr.clone()).unwrap();
        // Define what to do with the socket once it's obtained
        let action = socket.then(|sock| match sock {
            Ok((mut s, _)) => {
                let written = s.write(&[0x1, 0x2, 0x3]).unwrap();
                Ok(written)
            }
            Err(x) => Err(x),
        });
        // Execute the future in our event loop
        core.run(action).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv4() {
        let core = Core::new().unwrap();
        let tcp = TcpConfig::new(core.handle());

        let addr = "/ip4/127.0.0.1/tcp/0".parse::<Multiaddr>().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let (_, new_addr) = tcp.listen_on(addr).unwrap();
        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[test]
    fn replace_port_0_in_returned_multiaddr_ipv6() {
        let core = Core::new().unwrap();
        let tcp = TcpConfig::new(core.handle());

        let addr: Multiaddr = "/ip6/::1/tcp/0".parse().unwrap();
        assert!(addr.to_string().contains("tcp/0"));

        let (_, new_addr) = tcp.listen_on(addr).unwrap();
        assert!(!new_addr.to_string().contains("tcp/0"));
    }

    #[test]
    fn larger_addr_denied() {
        let core = Core::new().unwrap();
        let tcp = TcpConfig::new(core.handle());

        let addr = "/ip4/127.0.0.1/tcp/12345/tcp/12345"
            .parse::<Multiaddr>()
            .unwrap();
        assert!(tcp.listen_on(addr).is_err());
    }

    #[test]
    fn nat_traversal() {
        let core = Core::new().unwrap();
        let tcp = TcpConfig::new(core.handle());

        let server = "/ip4/127.0.0.1/tcp/10000".parse::<Multiaddr>().unwrap();
        let observed = "/ip4/80.81.82.83/tcp/25000".parse::<Multiaddr>().unwrap();

        let out = tcp.nat_traversal(&server, &observed);
        assert_eq!(
            out.unwrap(),
            "/ip4/80.81.82.83/tcp/10000".parse::<Multiaddr>().unwrap()
        );
    }
}
