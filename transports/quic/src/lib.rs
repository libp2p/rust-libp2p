// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! Implementation of the libp2p `Transport` trait for QUIC over UDP.

extern crate bytes;
extern crate fnv;
extern crate futures;
extern crate libp2p_core;
#[macro_use]
extern crate log;
extern crate multiaddr;
extern crate openssl;
extern crate parking_lot;
extern crate picoquic;
extern crate tokio_core;
extern crate tokio_io;

use bytes::BytesMut;
use fnv::FnvHashMap;
use futures::{future, future::FutureResult, prelude::*, Async, Poll};
use libp2p_core::{muxing::Shutdown, PeerId, PublicKey, StreamMuxer, Transport};
use multiaddr::{Multiaddr, Protocol};
use openssl::{
    error::ErrorStack,
    stack::StackRef,
    x509::{X509Ref, X509},
};
use parking_lot::Mutex;
use std::{
    cmp, fmt, io, iter,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    sync::Arc,
};
use tokio_core::reactor::Handle;

/// Represents the configuration for a QUIC transport capability for libp2p.
// TODO: add Default once the Handle is no longer needed
#[derive(Debug, Clone)]
pub struct QuicConfig {
    // tokio-core handle TODO: remove
    handle: Handle,
    /// RSA private key in DER format.
    private_key_der: Vec<u8>,
    /// Address to use when establishing an outgoing IPv4 connection. Port can be 0 for "any port".
    /// If the port is 0, it will be different for each outgoing connection.
    ipv4_src_addr: SocketAddrV4,
    /// Equivalent for `ipv4_src_addr` for IPv6.
    ipv6_src_addr: SocketAddrV6,
}

impl QuicConfig {
    /// Creates a new configuration object for QUIC.
    ///
    /// `rsa_private_key_der` must be the private RSA key to use in the connection in the DER
    /// format.
    #[inline]
    pub fn new(handle: Handle, rsa_private_key_der: Vec<u8>) -> QuicConfig {
        QuicConfig {
            handle,
            private_key_der: rsa_private_key_der,
            ipv4_src_addr: SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0),
            ipv6_src_addr: SocketAddrV6::new(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0), 0, 0, 0),
        }
    }

    /// Sets the source port to use for outgoing connections.
    ///
    /// If 0, means a different port for each new connection.
    #[inline]
    pub fn source_port(mut self, port: u16) -> Self {
        self.ipv4_src_addr.set_port(port);
        self.ipv6_src_addr.set_port(port);
        self
    }
}

impl Transport for QuicConfig {
    type Output = (PeerId, QuicMuxer);
    type Listener = QuicListenStream;
    type ListenerUpgrade = FutureResult<Self::Output, io::Error>;
    type Dial = QuicDialFut;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
        let listen_addr = match multiaddr_to_socketaddr(&addr) {
            Ok(sa) => sa,
            Err(_) => return Err((self, addr)),
        };

        let public_keys = Arc::new(Mutex::new(Default::default()));

        let mut quic_config = picoquic::Config::new();
        quic_config.set_private_key(self.private_key_der.clone(), picoquic::FileFormat::DER);
        quic_config.set_verify_certificate_handler(ListenCertifVerifier(public_keys.clone()));
        let context = picoquic::Context::new(&listen_addr, &self.handle, quic_config).unwrap(); // FIXME:

        let actual_addr = socket_addr_to_quic(context.local_addr());
        debug!(
            "Listening on {} ; actual_addr = {}",
            listen_addr, actual_addr
        );

        Ok((
            QuicListenStream {
                inner: context,
                public_keys,
            },
            actual_addr,
        ))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
        let target_addr = match multiaddr_to_socketaddr(&addr) {
            Ok(sa) => sa,
            Err(_) => return Err((self, addr)),
        };

        // As an optimization, we check that the address is not of the form `0.0.0.0`.
        // If so, we instantly refuse dialing instead of going through the kernel.
        if target_addr.port() == 0 || target_addr.ip().is_unspecified() {
            debug!("Instantly refusing dialing {}, as it is invalid", addr);
            return Err((self, addr));
        }

        debug!("Dialing {}", addr);

        let listen_addr = if target_addr.is_ipv4() {
            SocketAddr::from(self.ipv4_src_addr.clone())
        } else {
            SocketAddr::from(self.ipv6_src_addr.clone())
        };

        let public_key = Arc::new(Mutex::new(None));

        let mut quic_config = picoquic::Config::new();
        quic_config.set_private_key(self.private_key_der.clone(), picoquic::FileFormat::DER);
        quic_config.set_verify_certificate_handler(DialCertifVerifier(public_key.clone()));

        let mut context = picoquic::Context::new(&listen_addr, &self.handle, quic_config).unwrap(); // FIXME:
        let connec = context.new_connection(target_addr, String::new());

        Ok(QuicDialFut {
            inner: connec,
            public_key,
        })
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        // TODO: implement after https://github.com/libp2p/rust-libp2p/pull/550
        None
    }
}

/// An open connection. Implements `StreamMuxer`.
pub struct QuicMuxer {
    inner: Mutex<picoquic::Connection>,
}

impl fmt::Debug for QuicMuxer {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "QuicMuxer")
    }
}

/// A QUIC substream.
pub struct QuicMuxerSubstream {
    /// The actual stream from picoquic.
    inner: picoquic::Stream,
    /// Data waiting to be read.
    pending_read: BytesMut,
}

impl fmt::Debug for QuicMuxerSubstream {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "QuicMuxerSubstream")
    }
}

/// A QUIC substream being opened.
pub struct QuicMuxerOutboundSubstream {
    /// The actual stream from picoquic.
    inner: picoquic::NewStreamFuture,
}

impl fmt::Debug for QuicMuxerOutboundSubstream {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "QuicMuxerOutboundSubstream")
    }
}

impl StreamMuxer for QuicMuxer {
    type Substream = QuicMuxerSubstream;
    type OutboundSubstream = QuicMuxerOutboundSubstream;

    fn poll_inbound(&self) -> Poll<Option<Self::Substream>, io::Error> {
        match self.inner.lock().poll().map_err(convert_err)? {
            Async::Ready(Some(substream)) => Ok(Async::Ready(Some(QuicMuxerSubstream {
                inner: substream,
                pending_read: BytesMut::with_capacity(0),
            }))),
            Async::Ready(None) => Ok(Async::Ready(None)),
            Async::NotReady => Ok(Async::NotReady),
        }
    }

    #[inline]
    fn open_outbound(&self) -> Self::OutboundSubstream {
        QuicMuxerOutboundSubstream {
            inner: self.inner.lock().new_bidirectional_stream(),
        }
    }

    #[inline]
    fn poll_outbound(
        &self,
        substream: &mut Self::OutboundSubstream,
    ) -> Poll<Option<Self::Substream>, io::Error> {
        Ok(substream
            .inner
            .poll()
            .map_err(convert_err)?
            .map(|substream| {
                Some(QuicMuxerSubstream {
                    inner: substream,
                    pending_read: BytesMut::with_capacity(0),
                })
            }))
    }

    #[inline]
    fn destroy_outbound(&self, _: Self::OutboundSubstream) {}

    fn read_substream(
        &self,
        substream: &mut Self::Substream,
        buf: &mut [u8],
    ) -> Poll<usize, io::Error> {
        while substream.pending_read.is_empty() {
            match substream.inner.poll().map_err(convert_err)? {
                Async::Ready(Some(data)) => substream.pending_read = data,
                Async::Ready(None) => return Ok(Async::Ready(0)),
                Async::NotReady => return Ok(Async::NotReady),
            }
        }

        let to_read = cmp::min(buf.len(), substream.pending_read.len());
        buf.copy_from_slice(&substream.pending_read[..to_read]);
        substream.pending_read.split_at(to_read);
        Ok(Async::Ready(to_read))
    }

    #[inline]
    fn write_substream(
        &self,
        substream: &mut Self::Substream,
        buf: &[u8],
    ) -> Poll<usize, io::Error> {
        let len = buf.len();
        match substream
            .inner
            .start_send(From::from(buf.to_vec()))
            .map_err(convert_err)?
        {
            AsyncSink::Ready => Ok(Async::Ready(len)),
            AsyncSink::NotReady(_) => Ok(Async::NotReady),
        }
    }

    #[inline]
    fn flush_substream(&self, substream: &mut Self::Substream) -> Poll<(), io::Error> {
        substream.inner.poll_complete().map_err(convert_err)
    }

    fn shutdown_substream(&self, _: &mut Self::Substream, _: Shutdown) -> Poll<(), io::Error> {
        Ok(Async::Ready(()))
    }

    #[inline]
    fn destroy_substream(&self, _: Self::Substream) {}

    #[inline]
    fn shutdown(&self, _: Shutdown) -> Poll<(), io::Error> {
        Ok(Async::Ready(()))
    }

    fn flush_all(&self) -> Poll<(), io::Error> {
        // TODO: ?
        Ok(Async::Ready(()))
    }
}

/// If `addr` is a QUIC address, returns the corresponding `SocketAddr`.
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

/// Converts a `SocketAddr` into a QUIC multiaddr.
fn socket_addr_to_quic(addr: SocketAddr) -> Multiaddr {
    iter::once(Protocol::from(addr.ip()))
        .chain(iter::once(Protocol::Udp(addr.port())))
        .chain(iter::once(Protocol::Quic))
        .collect()
}

/// Future that dials an address.
#[must_use = "futures do nothing unless polled"]
pub struct QuicDialFut {
    inner: picoquic::NewConnectionFuture,
    public_key: Arc<Mutex<Option<PublicKey>>>,
}

impl fmt::Debug for QuicDialFut {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "QuicDialFut")
    }
}

impl Future for QuicDialFut {
    type Item = (PeerId, QuicMuxer);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll().map_err(convert_err)? {
            Async::Ready(stream) => {
                let public_key = self.public_key.lock().take().expect(
                    "The certificate validator is guaranteed to be called by picoquic \
                     and stores the public key in itself",
                );
                let peer_id = public_key.into_peer_id();
                let muxer = QuicMuxer {
                    inner: Mutex::new(stream),
                };
                Ok(Async::Ready((peer_id, muxer)))
            }
            Async::NotReady => Ok(Async::NotReady),
        }
    }
}

/// Implements `picoquic::VerifyCertificate`. Automatically accepts whatever certificate it
/// receives, and stores the public key in its inner variable.
struct DialCertifVerifier(Arc<Mutex<Option<PublicKey>>>);
impl picoquic::VerifyCertificate for DialCertifVerifier {
    fn verify(
        &mut self,
        _: picoquic::ConnectionId,
        _: picoquic::ConnectionType,
        cert: &X509Ref,
        _: &StackRef<X509>,
    ) -> Result<bool, ErrorStack> {
        let public_key = PublicKey::Rsa(cert.public_key()?.public_key_to_der()?);
        *self.0.lock() = Some(public_key);
        Ok(true)
    }
}

/// Stream that listens on an TCP/IP address.
#[must_use = "futures do nothing unless polled"]
pub struct QuicListenStream {
    inner: picoquic::Context,
    public_keys: Arc<Mutex<FnvHashMap<picoquic::ConnectionId, PublicKey>>>,
}

impl fmt::Debug for QuicListenStream {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "QuicListenStream")
    }
}

impl Stream for QuicListenStream {
    type Item = (
        future::FutureResult<(PeerId, QuicMuxer), io::Error>,
        Multiaddr,
    );
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, io::Error> {
        match self.inner.poll().map_err(convert_err)? {
            Async::Ready(Some(stream)) => {
                let public_key = self.public_keys.lock().remove(&stream.id()).expect(
                    "The certificate validator is guaranteed to be called by picoquic \
                     and stores the public key in itself",
                );

                let peer_id = public_key.into_peer_id();
                let addr = socket_addr_to_quic(stream.peer_addr());
                let muxer = QuicMuxer {
                    inner: Mutex::new(stream),
                };
                Ok(Async::Ready(Some((future::ok((peer_id, muxer)), addr))))
            }
            Async::Ready(None) => Ok(Async::Ready(None)),
            Async::NotReady => Ok(Async::NotReady),
        }
    }
}

/// Implements `picoquic::VerifyCertificate`. Automatically accepts whatever certificate it
/// receives, and stores the public key in its inner variable.
struct ListenCertifVerifier(Arc<Mutex<FnvHashMap<picoquic::ConnectionId, PublicKey>>>);
impl picoquic::VerifyCertificate for ListenCertifVerifier {
    fn verify(
        &mut self,
        id: picoquic::ConnectionId,
        _: picoquic::ConnectionType,
        cert: &X509Ref,
        _: &StackRef<X509>,
    ) -> Result<bool, ErrorStack> {
        let public_key = PublicKey::Rsa(cert.public_key()?.public_key_to_der()?);
        self.0.lock().insert(id, public_key);
        Ok(true)
    }
}

/// Converts a picoquic error into an IO error.
// TODO: eventually remove ; this is bad design
fn convert_err(error: picoquic::Error) -> io::Error {
    io::Error::new(io::ErrorKind::Other, error.to_string())
}
