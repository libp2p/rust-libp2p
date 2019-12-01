// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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

//! Implementation of the libp2p `Transport` trait for QUIC/UDP/IP.
//!
//! Uses [the *tokio* library](https://tokio.rs).
//!
//! # Usage
//!
//! Example:
//!
//! ```
//! extern crate libp2p_tcp;
//! use libp2p_tcp::QuicConfig;
//!
//! # fn main() {
//! let tcp = QuicConfig::new();
//! # }
//! ```
//!
//! The `QuicConfig` structs implements the `Transport` trait of the `swarm` library. See the
//! documentation of `swarm` and of libp2p in general to learn how to use the `Transport` trait.
//!
//! Note that QUIC provides transport, security, and multiplexing in a single protocol.  Therefore,
//! QUIC connections do not need to be upgraded.  You will get a compile-time error if you try.
//! Instead, you must pass all needed configuration into the constructor.

use futures::{
    future,
    prelude::*,
};
use ipnet::IpNet;
use libp2p_core::{
    multiaddr::{ip_to_multiaddr, Multiaddr, Protocol},
    transport::{ListenerEvent, TransportError},
    StreamMuxer, Transport,
};
use log::debug;
pub use quinn::{Endpoint, EndpointBuilder, EndpointError, ServerConfig};
use std::{
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll},
};

/// Represents the configuration for a QUIC/UDP/IP transport capability for libp2p.
///
/// The QUIC endpoints created by libp2p will need to be progressed by running the futures and streams
/// obtained by libp2p through the tokio reactor.
#[derive(Debug, Clone)]
pub struct QuicConfig {
    /// The underlying QUIC transport config.  Quinn provides functions for creating a suitable
    /// one.
    pub endpoint_builder: EndpointBuilder,
    /// The underlying QUIC transport endpoint.
    endpoint: Option<Endpoint>,
    /// The server configuration.  Quinn provides functions for making one.
    pub server_configuration: ServerConfig,
}

/// An error in the QUIC transport
#[derive(Debug, err_derive::Error)]
pub enum QuicError {
    /// An I/O error
    #[error(display = "Endpoint error: {}", _0)]
    EndpointError(#[source] quinn::EndpointError),
    #[error(display = "QUIC Protocol Error: {}", _0)]
    ConnectionError(#[source] quinn::ConnectionError),
    #[error(display = "QUIC outbound connection error: {}", _0)]
    ConnectError(#[source] quinn::ConnectError),
}

impl QuicConfig {
    /// Creates a new configuration object for TCP/IP.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self::new()
    }
}

pub struct QuicIncoming {
	/// This field uses structural pinning…
    incoming: quinn::Incoming,
	/// but this field does not.
    addr: Multiaddr,
}

type CompatConnecting = future::MapErr<quinn::Connecting, fn(quinn::ConnectionError) -> QuicError>;

impl futures_core::stream::Stream for QuicIncoming {
    type Item = Result<ListenerEvent<CompatConnecting>, QuicError>;
    fn poll_next(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.incoming).poll_next(ctx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(upgrade)) => {
                let peer = upgrade.remote_address();
                Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
                    remote_addr: ip_to_multiaddr(
                        peer.ip(),
                        &[Protocol::Udp(peer.port()), Protocol::Quic],
                    ),
                    upgrade: upgrade.map_err(QuicError::ConnectionError as _),
                    local_addr: self.addr.clone(),
                })))
            }
            Poll::Ready(None) => Poll::Ready(None),
        }
    }
}

struct QuicMuxer {
    bi_streams: quinn::IncomingBiStreams,
    connection: quinn::Connection,
    driver: quinn::ConnectionDriver,
}

pub struct SyncQuicMuxer(Mutex<QuicMuxer>);

// FIXME: if quinn ever starts using `!Unpin` futures, this will require `unsafe` code.
// Will probably fall back to spawning the connection driver in this case 🙁
impl StreamMuxer for SyncQuicMuxer {
    type OutboundSubstream = quinn::OpenBi;
    type Substream = Mutex<(quinn::SendStream, quinn::RecvStream)>;
    type Error = std::io::Error;
    fn poll_inbound(&self, cx: &mut Context) -> Poll<Result<Self::Substream, Self::Error>> {
		let mut this = self.0.lock().unwrap();
        match Pin::new(&mut this.driver).poll(cx) {
            Poll::Ready(Ok(())) => unreachable!(
                "ConnectionDriver will not resolve as long as `self.bi_streams` is alive"
            ),
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e.into())),
            Poll::Pending => (),
        }
        match Pin::new(&mut this.bi_streams).poll_next(cx) {
			Poll::Ready(Some(stream)) => Poll::Ready(stream.map(Mutex::new).map_err(From::from)),
			Poll::Ready(None) => Poll::Ready(Err(quinn::ConnectionError::LocallyClosed.into())),
			Poll::Pending => Poll::Pending,
		}
    }
    fn open_outbound(&self) -> Self::OutboundSubstream {
        self.0.lock().unwrap().connection.open_bi()
    }
	fn destroy_substream(&self, _substream: Self::Substream) {
	}
	fn destroy_outbound(&self, _substream: Self::OutboundSubstream) {
	}
	fn is_remote_acknowledged(&self) -> bool {
		true
	}
	fn write_substream(&self, cx: &mut Context, substream: &mut Self::Substream, buf: &[u8]) -> Poll<Result<usize, Self::Error>> {
		Pin::new(&mut substream.lock().unwrap().0).poll_write(cx, buf)
	}
	fn poll_outbound(&self, cx: &mut Context, substream: &mut Self::OutboundSubstream) -> Poll<Result<Self::Substream, Self::Error>> {
		Pin::new(substream).poll(cx).map_err(From::from).map_ok(Mutex::new)
	}
	fn read_substream(&self, cx: &mut Context, substream: &mut Self::Substream, buf: &mut [u8]) -> Poll<Result<usize, Self::Error>> {
		Pin::new(&mut substream.lock().unwrap().1).poll_read(cx, buf)
	}
    fn shutdown_substream(&self, cx: &mut Context, substream: &mut Self::Substream) -> Poll<Result<(), Self::Error>> {
		Pin::new(&mut substream.lock().unwrap().0).poll_close(cx)
	}
    fn flush_substream(&self, cx: &mut Context, substream: &mut Self::Substream) -> Poll<Result<(), Self::Error>> {
		// The actual implementation does nothing.  This at least does something.
		self.write_substream(cx, substream, b"").map_ok(drop)
	}
    fn flush_all(&self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
		Poll::Ready(Ok(()))
	}
    fn close(&self, _cx: &mut Context) -> Poll<Result<(), Self::Error>> {
		Poll::Ready(Ok(self.0.lock().unwrap().connection.close(0u32.into(), b"")))
	}
}

impl Transport for QuicConfig {
    type Output = quinn::NewConnection;
    type Error = QuicError;
    type Listener = QuicIncoming;
    type ListenerUpgrade = CompatConnecting;
    type Dial = CompatConnecting;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let socket_addr = if let Ok(sa) = multiaddr_to_socketaddr(&addr) {
            sa
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        let (driver, _endpoint, incoming) = self
            .endpoint_builder
            .bind(&socket_addr)
            .map_err(|e| TransportError::Other(QuicError::EndpointError(e)))?;
        tokio::task::spawn(driver.map_err(drop));
        Ok(QuicIncoming { incoming, addr })
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let socket_addr = if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
            if socket_addr.port() == 0 || socket_addr.ip().is_unspecified() {
                debug!("Instantly refusing dialing {}, as it is invalid", addr);
                return Err(TransportError::Other(QuicError::EndpointError(
                    EndpointError::Socket(io::ErrorKind::ConnectionRefused.into()),
                )));
            }
            socket_addr
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        let (_driver, endpoint, _incoming) =
            self.endpoint_builder
                .bind(&([0u8; 16], 0u16).into())
                .map_err(|e| TransportError::Other(QuicError::EndpointError(e)))?;

        Ok(endpoint
            .connect(&socket_addr, &socket_addr.to_string())
            .map_err(QuicError::ConnectError)?
            .map_err(QuicError::ConnectionError as _))
    }
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Result<SocketAddr, ()> {
    let mut iter = addr.iter();
    let proto1 = iter.next().ok_or(())?;
    let proto2 = iter.next().ok_or(())?;
    let proto3 = iter.next().ok_or(())?;

    if iter.next().is_some() {
        return Err(());
    }

    match (proto1, proto2, proto3) {
        (Protocol::Ip4(ip), Protocol::Udp(port), Protocol::Quic) => {
            Ok(SocketAddr::new(ip.into(), port))
        }
        (Protocol::Ip6(ip), Protocol::Udp(port), Protocol::Quic) => {
            Ok(SocketAddr::new(ip.into(), port))
        }
        _ => Err(()),
    }
}
