use tokio_io::{AsyncRead, AsyncWrite};
use futures::{Sink, StartSend, Poll, Stream, Async, AsyncSink};
use rustls::TLSError;
use libp2p_core::Negotiated;
use bytes::BufMut;
use std::io::{Error as IoError, ErrorKind as IoErrorKind, Write};
use rustls::{RootCertStore, Session, NoClientAuth, AllowAnyAuthenticatedClient,
             AllowAnyAnonymousOrAuthenticatedClient};
use std::io::Read;
use tokio_threadpool::blocking;

pub struct FullCodec<S>
    where
        S: AsyncRead + AsyncWrite + Send + 'static,
{
    server_session: Option<rustls::ServerSession>,
    client_session: Option<rustls::ClientSession>,
    socket: S,
}

impl<S> FullCodec<S>
    where
        S: AsyncRead + AsyncWrite + Send + 'static,
{
    pub fn from_server(mut socket: S, mut session: rustls::ServerSession) -> Self {
        FullCodec {
            server_session: Some(session),
            client_session: None,
            socket,
        }
    }

    pub fn from_client(mut socket: S, mut session: rustls::ClientSession) -> Self {
        let client_stream = rustls::Stream::new(&mut session, &mut socket);
        FullCodec {
            server_session: None,
            client_session: Some(session),
            socket,
        }
    }

    fn as_server_stream(&mut self) -> Option<rustls::Stream<rustls::ServerSession, S>> {
        if let Some(ref mut server_session) = self.server_session {
            return Some(rustls::Stream::new(server_session, &mut self.socket));
        }
        None
    }

    fn as_client_stream(&mut self) -> Option<rustls::Stream<rustls::ClientSession, S>> {
        if let Some(ref mut client_session) = self.client_session {
            return Some(rustls::Stream::new(client_session, &mut self.socket));
        }
        None
    }
}

impl<S> Sink for FullCodec<S>
    where
        S: AsyncRead + AsyncWrite + Send + 'static,
{
    type SinkItem = Vec<u8>;
    type SinkError = IoError;

    #[inline]
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        println!("sending tls...");
        if let Some(ref mut stream) = self.as_server_stream() {
            let n = stream.write(&item).unwrap();
            println!("sent {}", n);
        } else if let Some(ref mut stream) = self.as_client_stream() {
            let n = stream.write(&item).unwrap();
            println!("sent {}", n);
        }

        Ok(AsyncSink::Ready)
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
    }

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        Ok(Async::Ready(()))
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
        println!("reading tls...");
        let mut plaintext = Vec::new();
        if let Some(ref mut stream) = self.as_server_stream() {
            let n = stream.read_to_end(&mut plaintext).unwrap();
            println!("read {}", n);
        } else if let Some(ref mut stream) = self.as_client_stream() {
            let n = stream.read_to_end(&mut plaintext).unwrap();
            println!("read {}", n);
        }
        Ok(Async::Ready(Some(plaintext)))
    }
}


