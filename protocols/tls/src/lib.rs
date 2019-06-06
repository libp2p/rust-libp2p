
//! The `tls` protocol is a middleware that will encrypt and decrypt communications going
//! through a socket (or anything that implements `AsyncRead + AsyncWrite`).

pub use self::error::TlsError;

use bytes::BytesMut;
use futures::stream::MapErr as StreamMapErr;
use tokio_io::{AsyncRead, AsyncWrite};
use log::debug;
use libp2p_core::{Keypair, PublicKey, identity, upgrade::{UpgradeInfo, InboundUpgrade, OutboundUpgrade, Negotiated}};
use futures::{Future, Poll, Sink, StartSend, Stream, Async};
use rw_stream_sink::RwStreamSink;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use tokio_io::codec::length_delimited;
use crate::codec::FullCodec;

mod error;
mod codec;
mod handshake;

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

    fn handshake<T>(self, socket: T) -> impl Future<Item = TlsOutput<T>, Error = TlsError>
    where
        T: AsyncRead + AsyncWrite + Send
    {
        TlsMiddleware::handshake(socket, self).map(|(stream_sink, pubkey)| {
            let mapped = stream_sink.map_err(map_err as fn(_) -> _);
            TlsOutput {
                stream: RwStreamSink::new(mapped),
                remote_key: pubkey
            }
        })
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

/// Output of the tls protocol.
pub struct TlsOutput<S>
where
    S: AsyncRead + AsyncWrite,
{
    /// The encrypted stream.
    pub stream: RwStreamSink<StreamMapErr<TlsMiddleware<S>, fn(IoError) -> IoError>>,
    pub remote_key: PublicKey,
}

#[inline]
fn map_err(err: IoError) -> IoError {
    debug!("error during tls handshake {:?}", err);
    IoError::new(IoErrorKind::InvalidData, err)
}

pub struct TlsMiddleware<S>
{
    inner: codec::FullCodec<S>,
}

impl<S> TlsMiddleware<S>
where
    S: AsyncRead + AsyncWrite + Send,
{
    pub fn handshake(socket: S, config: TlsConfig)
        -> impl Future<Item = (TlsMiddleware<S>, PublicKey), Error = TlsError> {
        handshake::handshake(socket, config).map(|(inner, pubkey)| {
            (TlsMiddleware{ inner }, pubkey)
        })
    }
}

impl<S> Sink for TlsMiddleware<S>
where
    S: AsyncRead + AsyncWrite,
{
    type SinkItem = BytesMut;
    type SinkError = IoError;

    #[inline]
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        println!("middleware sink: {}", item.len());
        self.inner.start_send(item)
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.poll_complete()
    }

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.inner.close()
    }
}

impl<S> Stream for TlsMiddleware<S>
where
    S: AsyncRead + AsyncWrite,
{
    type Item = BytesMut;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.inner.poll()
    }
}
