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

use crate::{Transport, transport::{TransportError, ListenerEvent}};
use bytes::{Bytes, IntoBuf};
use fnv::FnvHashMap;
use futures::{future::{self, FutureResult}, prelude::*, sync::mpsc, try_ready};
use lazy_static::lazy_static;
use multiaddr::{Protocol, Multiaddr};
use parking_lot::Mutex;
use rw_stream_sink::RwStreamSink;
use std::{collections::hash_map::Entry, error, fmt, io, num::NonZeroU64};

lazy_static! {
    static ref HUB: Mutex<FnvHashMap<NonZeroU64, mpsc::Sender<Channel<Bytes>>>> =
        Mutex::new(FnvHashMap::default());
}

/// Transport that supports `/memory/N` multiaddresses.
#[derive(Debug, Copy, Clone, Default)]
pub struct MemoryTransport;

/// Connection to a `MemoryTransport` currently being opened.
pub struct DialFuture {
    sender: mpsc::Sender<Channel<Bytes>>,
    channel_to_send: Option<Channel<Bytes>>,
    channel_to_return: Option<Channel<Bytes>>,
}

impl Future for DialFuture {
    type Item = Channel<Bytes>;
    type Error = MemoryTransportError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(c) = self.channel_to_send.take() {
            match self.sender.start_send(c) {
                Err(_) => return Err(MemoryTransportError::Unreachable),
                Ok(AsyncSink::NotReady(t)) => {
                    self.channel_to_send = Some(t);
                    return Ok(Async::NotReady)
                },
                _ => (),
            }
        }
        match self.sender.close() {
            Err(_) => Err(MemoryTransportError::Unreachable),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(_)) => Ok(Async::Ready(self.channel_to_return.take()
                .expect("Future should not be polled again once complete"))),
        }
    }
}

impl Transport for MemoryTransport {
    type Output = Channel<Bytes>;
    type Error = MemoryTransportError;
    type Listener = Listener;
    type ListenerUpgrade = FutureResult<Self::Output, Self::Error>;
    type Dial = DialFuture;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        let port = if let Ok(port) = parse_memory_addr(&addr) {
            port
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        let mut hub = (&*HUB).lock();

        let port = if let Some(port) = NonZeroU64::new(port) {
            port
        } else {
            loop {
                let port = match NonZeroU64::new(rand::random()) {
                    Some(p) => p,
                    None => continue,
                };
                if !hub.contains_key(&port) {
                    break port;
                }
            }
        };


        let (tx, rx) = mpsc::channel(2);
        match hub.entry(port) {
            Entry::Occupied(_) =>
                return Err(TransportError::Other(MemoryTransportError::Unreachable)),
            Entry::Vacant(e) => e.insert(tx)
        };

        let listener = Listener {
            port,
            addr: Protocol::Memory(port.get()).into(),
            receiver: rx,
            tell_listen_addr: true
        };

        Ok(listener)
    }

    fn dial(self, addr: Multiaddr) -> Result<DialFuture, TransportError<Self::Error>> {
        let port = if let Ok(port) = parse_memory_addr(&addr) {
            if let Some(port) = NonZeroU64::new(port) {
                port
            } else {
                return Err(TransportError::Other(MemoryTransportError::Unreachable));
            }
        } else {
            return Err(TransportError::MultiaddrNotSupported(addr));
        };

        let hub = HUB.lock();
        if let Some(sender) = hub.get(&port) {
            let (a_tx, a_rx) = mpsc::channel(4096);
            let (b_tx, b_rx) = mpsc::channel(4096);
            Ok(DialFuture {
                sender: sender.clone(),
                channel_to_send: Some(RwStreamSink::new(Chan { incoming: a_rx, outgoing: b_tx })),
                channel_to_return: Some(RwStreamSink::new(Chan { incoming: b_rx, outgoing: a_tx })),

            })
        } else {
            Err(TransportError::Other(MemoryTransportError::Unreachable))
        }
    }
}

/// Error that can be produced from the `MemoryTransport`.
#[derive(Debug, Copy, Clone)]
pub enum MemoryTransportError {
    /// There's no listener on the given port.
    Unreachable,
    /// Tries to listen on a port that is already in use.
    AlreadyInUse,
}

impl fmt::Display for MemoryTransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            MemoryTransportError::Unreachable => write!(f, "No listener on the given port."),
            MemoryTransportError::AlreadyInUse => write!(f, "Port already occupied."),
        }
    }
}

impl error::Error for MemoryTransportError {}

/// Listener for memory connections.
pub struct Listener {
    /// Port we're listening on.
    port: NonZeroU64,
    /// The address we are listening on.
    addr: Multiaddr,
    /// Receives incoming connections.
    receiver: mpsc::Receiver<Channel<Bytes>>,
    /// Generate `ListenerEvent::NewAddress` to inform about our listen address.
    tell_listen_addr: bool
}

impl Stream for Listener {
    type Item = ListenerEvent<FutureResult<Channel<Bytes>, MemoryTransportError>>;
    type Error = MemoryTransportError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.tell_listen_addr {
            self.tell_listen_addr = false;
            return Ok(Async::Ready(Some(ListenerEvent::NewAddress(self.addr.clone()))))
        }
        let channel = try_ready!(Ok(self.receiver.poll()
            .expect("Life listeners always have a sender.")));
        let channel = match channel {
            Some(c) => c,
            None => return Ok(Async::Ready(None))
        };
        let event = ListenerEvent::Upgrade {
            upgrade: future::ok(channel),
            local_addr: self.addr.clone(),
            remote_addr: Protocol::Memory(self.port.get()).into()
        };
        Ok(Async::Ready(Some(event)))
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let val_in = HUB.lock().remove(&self.port);
        debug_assert!(val_in.is_some());
    }
}

/// If the address is `/memory/n`, returns the value of `n`.
fn parse_memory_addr(a: &Multiaddr) -> Result<u64, ()> {
    let mut iter = a.iter();

    let port = if let Some(Protocol::Memory(port)) = iter.next() {
        port
    } else {
        return Err(());
    };

    if iter.next().is_some() {
        return Err(());
    }

    Ok(port)
}

/// A channel represents an established, in-memory, logical connection between two endpoints.
///
/// Implements `AsyncRead` and `AsyncWrite`.
pub type Channel<T> = RwStreamSink<Chan<T>>;

/// A channel represents an established, in-memory, logical connection between two endpoints.
///
/// Implements `Sink` and `Stream`.
pub struct Chan<T = Bytes> {
    incoming: mpsc::Receiver<T>,
    outgoing: mpsc::Sender<T>,
}

impl<T> Stream for Chan<T> {
    type Item = T;
    type Error = io::Error;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.incoming.poll().map_err(|()| io::ErrorKind::BrokenPipe.into())
    }
}

impl<T> Sink for Chan<T> {
    type SinkItem = T;
    type SinkError = io::Error;

    #[inline]
    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        self.outgoing.start_send(item).map_err(|_| io::ErrorKind::BrokenPipe.into())
    }

    #[inline]
    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        self.outgoing.poll_complete().map_err(|_| io::ErrorKind::BrokenPipe.into())
    }

    #[inline]
    fn close(&mut self) -> Poll<(), Self::SinkError> {
        self.outgoing.close().map_err(|_| io::ErrorKind::BrokenPipe.into())
    }
}

impl<T: IntoBuf> Into<RwStreamSink<Chan<T>>> for Chan<T> {
    #[inline]
    fn into(self) -> RwStreamSink<Chan<T>> {
        RwStreamSink::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_memory_addr_works() {
        assert_eq!(parse_memory_addr(&"/memory/5".parse().unwrap()), Ok(5));
        assert_eq!(parse_memory_addr(&"/tcp/150".parse().unwrap()), Err(()));
        assert_eq!(parse_memory_addr(&"/memory/0".parse().unwrap()), Ok(0));
        assert_eq!(parse_memory_addr(&"/memory/5/tcp/150".parse().unwrap()), Err(()));
        assert_eq!(parse_memory_addr(&"/tcp/150/memory/5".parse().unwrap()), Err(()));
        assert_eq!(parse_memory_addr(&"/memory/1234567890".parse().unwrap()), Ok(1_234_567_890));
    }

    #[test]
    fn listening_twice() {
        let transport = MemoryTransport::default();
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_ok());
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_ok());
        let _listener = transport.listen_on("/memory/1639174018481".parse().unwrap()).unwrap();
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_err());
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_err());
        drop(_listener);
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_ok());
        assert!(transport.listen_on("/memory/1639174018481".parse().unwrap()).is_ok());
    }

    #[test]
    fn port_not_in_use() {
        let transport = MemoryTransport::default();
        assert!(transport.dial("/memory/810172461024613".parse().unwrap()).is_err());
        let _listener = transport.listen_on("/memory/810172461024613".parse().unwrap()).unwrap();
        assert!(transport.dial("/memory/810172461024613".parse().unwrap()).is_ok());
    }

    // TODO: test that is actually works
}
