
//! The `secio` protocol is a middleware that will encrypt and decrypt communications going
//! through a socket (or anything that implements `AsyncRead + AsyncWrite`).

pub use self::error::TlsError;

use futures::stream::MapErr as StreamMapErr;
use tokio_io::{AsyncRead, AsyncWrite};
use libp2p_core::{PublicKey, identity, upgrade::{UpgradeInfo, InboundUpgrade, OutboundUpgrade, Negotiated}};
use futures::{Future, Poll, Sink, StartSend, Stream};
use rw_stream_sink::RwStreamSink;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;

mod error;

/// Implementation of the `ConnectionUpgrade` trait of `libp2p_core`. Automatically applies
/// tls on any connection.
#[derive(Clone)]
pub struct TlsConfig {
    /// Private and public keys of the local node.
    pub(crate) key: identity::Keypair,
    pub(crate) agreements_prop: Option<String>,
    pub(crate) ciphers_prop: Option<String>,
    pub(crate) digests_prop: Option<String>
}

impl TlsConfig {
    pub fn new(kp: identity::Keypair) -> Self {
        TlsConfig {
            key: kp,
            agreements_prop: None,
            ciphers_prop: None,
            digests_prop: None
        }
    }
}

impl UpgradeInfo for TlsConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/tls/0.1.0")
    }
}

impl <T> InboundUpgrade<T> for TlsConfig
    where T: AsyncRead + AsyncWrite + Send + 'static,
{
    type Output = TlsOutput<Negotiated<T>>;
    type Error = TlsError;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_inbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        Box::new(self.handshake(socket))
    }
}

impl<T> OutboundUpgrade<T> for TlsConfig
where
    T: AsyncRead + AsyncWrite + Send + 'static,
{
    type Output = TlsOutput<Negotiated<T>>;
    type Error = TlsError;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_outbound(self, socket: Negotiated<T>, _: Self::Info) -> Self::Future {
        Box::new(self.handshake(socket))
    }
}

/// Output of the secio protocol.
pub struct TlsOutput<S>
where
    S: AsyncRead + AsyncWrite,
{
    /// The encrypted stream.
    pub stream: RwStreamSink<StreamMapErr<TlsMiddleware<S>, fn(TlsError) -> IoError>>,
    /// The public key of the remote.
    pub remote_key: PublicKey,
    /// Ephemeral public key used during the negotiation.
    pub ephemeral_public_key: Vec<u8>,
}

impl<S> TlsMiddleware<S>
where
    S: AsyncRead + AsyncWrite + Send,
{
    /*
    pub fn handshake(socket: S, config: TlsConfig)
        -> Impl Future<Item = (TlsMiddleware<S>, PublicKey, Vec<u8>), Error = TlsError>
    {

    }
    */
}

impl<S> Sink for TlsMiddleware<S>
where
    S: AsyncRead + AsyncWrite,
{
    type SinkItem = BytesMut;
    type SinkError = IoError;

    #[inline]
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
    }

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
    }
}

impl<S> Stream for TlsMiddleWare<S>
where
    S: AsyncRead + AsyncWrite
{
    type Item = Vec<u8>;
    type Error = TlsError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::item>, Self::Error> {
    }
}
