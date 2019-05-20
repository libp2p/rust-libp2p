// Copyright 2019 Parity Technologies (UK) Ltd.
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

use bytes::BytesMut;
use crate::error::Error;
use futures::{future::{self, Either}, prelude::*, try_ready};
use libp2p_core::{
    Transport,
    multiaddr::{Protocol, Multiaddr},
    transport::{ListenerEvent, TransportError}
};
use log::{debug, trace};
use soketto::{base, connection::{Connection, Mode}, handshake};
use std::io;
use tokio_codec::{Framed, FramedParts};
use tokio_io::{AsyncRead, AsyncWrite};

const MAX_DATA_SIZE: u64 = 256 * 1024 * 1024;

/// A Websocket transport whose output type is a [`Stream`] and [`Sink`] of
/// frame payloads (text or binary) but does not implement [`AsyncRead`] or
/// [`AsyncWrite`]. See [`WsConfig`] if you require the latter.
#[derive(Debug, Clone)]
pub struct WsConfig<T> {
    transport: T,
    max_data_size: u64
}

impl<T> WsConfig<T>
where
    T: Transport,
    T::Error: Send + 'static,
    T::Dial: Send + 'static,
    T::Listener: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Output: AsyncRead + AsyncWrite + Send + 'static
{
    pub fn new(transport: T) -> Self {
        WsConfig {
            transport,
            max_data_size: MAX_DATA_SIZE
        }
    }

    pub fn max_data_size(&self) -> u64 {
        self.max_data_size
    }

    pub fn set_max_data_size(&mut self, size: u64) -> &mut Self {
        self.max_data_size = size;
        self
    }
}

impl<T> Transport for WsConfig<T>
where
    T: Transport,
    T::Error: Send + 'static,
    T::Dial: Send + 'static,
    T::Listener: Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Output: AsyncRead + AsyncWrite + Send + 'static
{
    type Output = BytesConnection<T::Output>;
    type Error = Error<T::Error>;
    type Listener = Box<dyn Stream<Item = ListenerEvent<Self::ListenerUpgrade>, Error = Self::Error> + Send>;
    type ListenerUpgrade = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;
    type Dial = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let mut inner_addr = addr.clone();
        if Some(Protocol::Ws) != inner_addr.pop() {
            return Err(TransportError::MultiaddrNotSupported(addr))
        }
        let max_size = self.max_data_size;
        let listen = self.transport.listen_on(inner_addr)
            .map_err(|e| e.map(Error::Transport))?
            .map_err(Error::Transport)
            .map(move |event| match event {
                ListenerEvent::NewAddress(mut a) => {
                    a = a.with(Protocol::Ws);
                    debug!("Listening on {}", a);
                    ListenerEvent::NewAddress(a)
                }
                ListenerEvent::AddressExpired(mut a) => {
                    a = a.with(Protocol::Ws);
                    ListenerEvent::AddressExpired(a)
                }
                ListenerEvent::Upgrade { upgrade, mut listen_addr, mut remote_addr } => {
                    listen_addr = listen_addr.with(Protocol::Ws);
                    remote_addr = remote_addr.with(Protocol::Ws);
                    let upgraded = upgrade.map_err(Error::Transport).and_then(move |stream| {
                        debug!("Incoming connection");
                        Framed::new(stream, handshake::Server::new())
                            .into_future()
                            .map_err(|(e, _framed)| Error::Handshake(Box::new(e)))
                            .and_then(move |(request, framed)| {
                                if let Some(r) = request {
                                    trace!("accepting websocket handshake request");
                                    let key = Vec::from(r.key());
                                    Either::A(framed.send(Ok(handshake::Accept::new(key)))
                                        .map_err(|e| Error::Base(Box::new(e)))
                                        .and_then(move |framed| {
                                            let mut codec = base::Codec::new();
                                            codec.set_max_data_size(max_size);
                                            let old = framed.into_parts();
                                            let mut new = FramedParts::new(old.io, codec);
                                            new.read_buf = old.read_buf;
                                            new.write_buf = old.write_buf;
                                            let framed = Framed::from_parts(new);
                                            let conn = Connection::from_framed(framed, Mode::Server);
                                            Ok(BytesConnection(conn))
                                        }))
                                } else {
                                    debug!("connection terminated during handshake");
                                    let e: io::Error = io::ErrorKind::ConnectionAborted.into();
                                    Either::B(future::err(Error::Handshake(Box::new(e))))
                                }
                            })
                    });
                    ListenerEvent::Upgrade {
                        upgrade: Box::new(upgraded) as Box<dyn Future<Item = _, Error = _> + Send>,
                        listen_addr,
                        remote_addr
                    }
                }
            });
        Ok(Box::new(listen) as Box<_>)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let mut inner_addr = addr.clone();

        if let Some(Protocol::Ws) | Some(Protocol::Wss) = inner_addr.pop() {
            // OK
        } else {
            trace!("{} is not a websocket multiaddr", addr);
            return Err(TransportError::MultiaddrNotSupported(addr));
        }

        let host = {
            let mut iter = addr.iter();
            let first = iter.next();
            let second = iter.next();
            match (first, second) {
                (Some(Protocol::Ip4(ip)), Some(Protocol::Tcp(port))) => format!("{}:{}", ip, port),
                (Some(Protocol::Ip6(ip)), Some(Protocol::Tcp(port))) => format!("{}:{}", ip, port),
                (Some(Protocol::Dns4(h)), Some(Protocol::Tcp(port))) => format!("{}:{}", h, port),
                (Some(Protocol::Dns6(h)), Some(Protocol::Tcp(port))) => format!("{}:{}", h, port),
                _ => return Err(TransportError::MultiaddrNotSupported(addr))
            }
        };

        debug!("Dialing {} through inner transport", inner_addr);

        let max_size = self.max_data_size;
        let dial = self.transport.dial(inner_addr).map_err(|e| e.map(Error::Transport))?
            .map_err(Error::Transport)
            .and_then(move |stream| {
                let client = handshake::Client::new(host, "/");
                Framed::new(stream, client)
                    .send(())
                    .map_err(|e| Error::Handshake(Box::new(e)))
                    .and_then(|framed| {
                        framed.into_future().map_err(|(e, _)| Error::Base(Box::new(e)))
                    })
                    .and_then(move |(response, framed)| {
                        if response.is_none() {
                            debug!("connection terminated during handshake");
                            let e: io::Error = io::ErrorKind::ConnectionAborted.into();
                            return Err(Error::Handshake(Box::new(e)))
                        }
                        let mut codec = base::Codec::new();
                        codec.set_max_data_size(max_size);
                        let old = framed.into_parts();
                        let mut new = FramedParts::new(old.io, codec);
                        new.read_buf = old.read_buf;
                        new.write_buf = old.write_buf;
                        let framed = Framed::from_parts(new);
                        let conn = Connection::from_framed(framed, Mode::Client);
                        Ok(BytesConnection(conn))
                    })
            });

        Ok(Box::new(dial) as Box<_>)
    }
}

// BytesConnection ////////////////////////////////////////////////////////////////////////////////

/// A [`Stream`] and [`Sink`] that produces and consumes [`BytesMut`] values
/// which correspond to the payload data of websocket frames.
#[derive(Debug)]
pub struct BytesConnection<T>(Connection<T>);

impl<T: AsyncRead + AsyncWrite> Stream for BytesConnection<T> {
    type Item = BytesMut;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let data = try_ready!(self.0.poll().map_err(|e| io::Error::new(io::ErrorKind::Other, e)));
        Ok(Async::Ready(data.map(base::Data::into_bytes)))
    }
}

impl<T: AsyncRead + AsyncWrite> Sink for BytesConnection<T> {
    type SinkItem = BytesMut;
    type SinkError = io::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        let result = self.0.start_send(base::Data::Binary(item))
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e));

        if let AsyncSink::NotReady(data) = result? {
            Ok(AsyncSink::NotReady(data.into_bytes()))
        } else {
            Ok(AsyncSink::Ready)
        }
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.0.poll_complete().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.0.close().map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }
}

