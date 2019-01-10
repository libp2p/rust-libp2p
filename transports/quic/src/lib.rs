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

use bytes::BytesMut;
use failure::{Compat, Fail};
use fnv::FnvHashMap;
use futures::{future, future::FutureResult, prelude::*, Async, Poll};
use libp2p_core::{muxing::Shutdown, PeerId, PublicKey, StreamMuxer, Transport, TransportError};
use log::{debug, warn};
use multiaddr::{Multiaddr, Protocol};
use openssl::{error::ErrorStack, pkey, rsa::Rsa, stack::StackRef, x509::{X509Ref, X509}};
use parking_lot::Mutex;
use picoquic;
use std::{
    cmp, fmt, io, iter,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    sync::Arc
};
use tokio_executor::{Executor, SpawnError};

#[derive(Clone)]
pub struct SecretKey {
    rsa: Rsa<pkey::Private>
}

impl SecretKey {
    pub fn pem(k: &[u8]) -> Result<Self, QuicError> {
        Ok(SecretKey {
            rsa: Rsa::private_key_from_pem(k)?
        })
    }

    pub fn der(k: &[u8]) -> Result<Self, QuicError> {
        Ok(SecretKey {
            rsa: Rsa::private_key_from_der(k)?
        })
    }

    pub fn to_der(&self) -> Result<Vec<u8>, QuicError> {
        Ok(self.rsa.private_key_to_der()?)
    }

    pub fn to_pem(&self) -> Result<Vec<u8>, QuicError> {
        Ok(self.rsa.private_key_to_pem()?)
    }

    pub fn public_key(&self) -> Result<PublicKey, QuicError> {
        Ok(self.rsa.public_key_to_der().map(PublicKey::Rsa)?)
    }
}

/// Represents the configuration for a QUIC transport capability for libp2p.
#[derive(Clone)]
pub struct QuicConfig {
    executor: Exec,
    /// RSA private key
    private_key: SecretKey,
    /// Address to use when establishing an outgoing IPv4 connection. Port can be 0 for "any port".
    /// If the port is 0, it will be different for each outgoing connection.
    ipv4_src_addr: SocketAddrV4,
    /// Equivalent for `ipv4_src_addr` for IPv6.
    ipv6_src_addr: SocketAddrV6,
}

impl fmt::Debug for QuicConfig {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("QuicConfig")
            .field("ipv4_src_addr", &self.ipv4_src_addr)
            .field("ipv6_src_addr", &self.ipv6_src_addr)
            .finish()
    }
}

impl QuicConfig {
    /// Creates a new configuration object for QUIC.
    pub fn new(e: impl Executor + Send + 'static, key: SecretKey) -> Self {
        QuicConfig {
            executor: Exec { inner: Arc::new(Mutex::new(e))},
            private_key: key,
            ipv4_src_addr: SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0),
            ipv6_src_addr: SocketAddrV6::new(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0), 0, 0, 0),
        }
    }

    /// Sets the source port to use for outgoing connections.
    ///
    /// If 0, means a different port for each new connection.
    pub fn source_port(mut self, port: u16) -> Self {
        self.ipv4_src_addr.set_port(port);
        self.ipv6_src_addr.set_port(port);
        self
    }
}

impl Transport for QuicConfig {
    type Output = (PeerId, QuicMuxer);
    type Error = QuicError;
    type Listener = QuicListenStream;
    type ListenerUpgrade = FutureResult<Self::Output, QuicError>;
    type Dial = QuicDialFut;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>> {
        let listen_addr = match multiaddr_to_socketaddr(&addr) {
            Ok(sa) => sa,
            Err(_) => return Err(TransportError::MultiaddrNotSupported(addr))
        };

        let public_keys = Arc::new(Mutex::new(Default::default()));

        let mut quic_config = picoquic::Config::new();
        let der = self.private_key.to_der().map_err(Into::into)?;
        quic_config.set_private_key(der, picoquic::FileFormat::DER);
        quic_config.set_verify_certificate_handler(ListenCertifVerifier(public_keys.clone()));

        let context = picoquic::Context::new(&listen_addr, self.executor.clone(), quic_config)
            .map_err(|e| TransportError::Other(e.into()))?;

        let actual_addr = socket_addr_to_quic(context.local_addr());
        debug!("Listening on {}; actual_addr = {}", listen_addr, actual_addr);

        Ok((QuicListenStream { inner: context, public_keys }, actual_addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let target_addr = match multiaddr_to_socketaddr(&addr) {
            Ok(sa) => sa,
            Err(_) => return Err(TransportError::MultiaddrNotSupported(addr))
        };

        // As an optimization, we check that the address is not of the form `0.0.0.0`.
        // If so, we instantly refuse dialing instead of going through the kernel.
        if target_addr.port() == 0 || target_addr.ip().is_unspecified() {
            debug!("Instantly refusing dialing {}, as it is invalid", addr);
            return Err(TransportError::MultiaddrNotSupported(addr))
        }

        debug!("Dialing {}", addr);

        let listen_addr = if target_addr.is_ipv4() {
            SocketAddr::from(self.ipv4_src_addr.clone())
        } else {
            SocketAddr::from(self.ipv6_src_addr.clone())
        };

        let public_key = Arc::new(Mutex::new(None));

        let mut quic_config = picoquic::Config::new();
        let der = self.private_key.to_der().map_err(Into::into)?;
        quic_config.set_private_key(der, picoquic::FileFormat::DER);
        quic_config.set_verify_certificate_handler(DialCertifVerifier(public_key.clone()));

        let mut context = picoquic::Context::new(&listen_addr, self.executor.clone(), quic_config)
            .map_err(|e| TransportError::Other(e.into()))?;

        let connec = context.new_connection(target_addr, String::new());

        Ok(QuicDialFut { context: Some(context), inner: connec, public_key })
    }

    fn nat_traversal(&self, _server: &Multiaddr, _observed: &Multiaddr) -> Option<Multiaddr> {
        // TODO: implement after https://github.com/libp2p/rust-libp2p/pull/550
        None
    }
}

#[derive(Clone)]
struct Exec {
    inner: Arc<Mutex<dyn Executor + Send>>
}

impl Executor for Exec {
    fn spawn(&mut self, fut: Box<dyn Future<Item=(), Error=()> + Send>) -> Result<(), SpawnError> {
        self.inner.lock().spawn(fut)
    }
}

/// An open connection. Implements `StreamMuxer`.
pub struct QuicMuxer {
    _context: Option<picoquic::Context>,
    inner: Mutex<picoquic::Connection>,
}

impl fmt::Debug for QuicMuxer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("QuicMuxer")
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
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("QuicMuxerSubstream")
    }
}

/// A QUIC substream being opened.
pub struct QuicMuxerOutboundSubstream {
    /// The actual stream from picoquic.
    inner: picoquic::NewStreamFuture,
}

impl fmt::Debug for QuicMuxerOutboundSubstream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("QuicMuxerOutboundSubstream")
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
    context: Option<picoquic::Context>,
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
    type Error = QuicError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(stream)) => {
                let public_key = self.public_key.lock().take().expect(
                    "The certificate validator is guaranteed to be called by picoquic \
                     and stores the public key in itself",
                );
                let peer_id = public_key.into_peer_id();
                let muxer = QuicMuxer {
                    _context: self.context.take(),
                    inner: Mutex::new(stream),
                };
                Ok(Async::Ready((peer_id, muxer)))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(e) => {
                warn!("dial error: {}", e);
                Err(e.into())
            }
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
        future::FutureResult<(PeerId, QuicMuxer), QuicError>,
        Multiaddr,
    );
    type Error = QuicError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(Some(stream))) => {
                let public_key = self.public_keys.lock().remove(&stream.id()).expect(
                    "The certificate validator is guaranteed to be called by picoquic \
                     and stores the public key in itself",
                );

                let peer_id = public_key.into_peer_id();
                let addr = socket_addr_to_quic(stream.peer_addr());
                let muxer = QuicMuxer {
                    _context: None,
                    inner: Mutex::new(stream),
                };
                Ok(Async::Ready(Some((future::ok((peer_id, muxer)), addr))))
            }
            Ok(Async::Ready(None)) => Ok(Async::Ready(None)),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(e) => {
                warn!("listen error: {}", e);
                Err(e.into())
            }
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

#[derive(Debug)]
pub enum QuicError {
    Io(io::Error),
    PicoQuic(Compat<picoquic::Error>),
    OpenSsl(ErrorStack),
    #[doc(hidden)]
    __Nonexhaustive
}

impl fmt::Display for QuicError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            QuicError::Io(e) => write!(f, "i/o: {}", e),
            QuicError::PicoQuic(e) => write!(f, "picoquic: {}", e),
            QuicError::OpenSsl(e) => write!(f, "openssl: {}", e),
            QuicError::__Nonexhaustive => f.write_str("__Nonexhaustive")
        }
    }
}

impl std::error::Error for QuicError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            QuicError::Io(e) => Some(e),
            QuicError::PicoQuic(e) => Some(e),
            QuicError::OpenSsl(e) => Some(e),
            QuicError::__Nonexhaustive => None
        }
    }
}

impl From<io::Error> for QuicError {
    fn from(e: io::Error) -> Self {
        QuicError::Io(e)
    }
}

impl From<ErrorStack> for QuicError {
    fn from(e: ErrorStack) -> Self {
        QuicError::OpenSsl(e)
    }
}

impl From<picoquic::Error> for QuicError {
    fn from(e: picoquic::Error) -> Self {
        QuicError::PicoQuic(e.compat())
    }
}

impl Into<TransportError<Self>> for QuicError {
    fn into(self) -> TransportError<Self> {
        TransportError::Other(self)
    }
}

