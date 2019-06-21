use self::encode::EncoderMiddleware;
use self::decode::DecoderMiddleware;
use tokio_io::codec::length_delimited;
use tokio_io::{AsyncRead, AsyncWrite};
use futures::{Sink, StartSend, Poll, Stream, Async, AsyncSink};
use rustls::TLSError;
use libp2p_core::Negotiated;
use bytes::BufMut;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use rustls::{RootCertStore, Session, NoClientAuth, AllowAnyAuthenticatedClient,
             AllowAnyAnonymousOrAuthenticatedClient};
use std::io::Read;

pub mod encode;
mod decode;

pub struct FullCodec<S>
    where
        S: AsyncRead + AsyncWrite + Send,
{
    socket: S,
    tls_client: rustls::ClientSession,
    tls_server: rustls::ServerSession,
}

impl<S> FullCodec<S>
    where
        S: AsyncRead + AsyncWrite + Send,
{
   pub fn new(socket: S, tls_client: rustls::ClientSession, tls_server: rustls::ServerSession) -> Self {
     FullCodec {
         socket,
         tls_client,
         tls_server,
     }
   }
}

impl<S> Sink for FullCodec<S>
where
    S: AsyncRead + AsyncWrite + Send,
{
    type SinkItem = Vec<u8>;
    type SinkError = IoError;

    #[inline]
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        //self.raw_stream.start_send(item)
        Ok(AsyncSink::Ready)
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        //self.raw_stream.poll_complete()
        Ok(Async::NotReady)
    }

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        //self.raw_stream.close()
        Ok(Async::NotReady)
    }
}

impl<S> Stream for FullCodec<S>
where
    S: AsyncRead + AsyncWrite + Send,
{
    type Item = Vec<u8>;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.tls_client.wants_read() {
            self.tls_client.read_tls(&mut self.socket).unwrap();
            self.tls_client.process_new_packets().unwrap();

            let mut plaintext = Vec::new();
            self.tls_client.read_to_end(&mut plaintext).unwrap();
            ()
            //Ok(Async::Ready(Some(plaintext)))
        }

        Ok(Async::NotReady)
    }
}

