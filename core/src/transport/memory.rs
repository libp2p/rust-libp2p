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

use crate::{Transport, transport::TransportError};
use bytes::{Bytes, IntoBuf};
use fnv::FnvHashMap;
use futures::{future::{self, FutureResult}, prelude::*, sync::mpsc, try_ready};
use lazy_static::lazy_static;
use multiaddr::{Protocol, Multiaddr};
use parking_lot::Mutex;
use rw_stream_sink::RwStreamSink;
use std::{collections::hash_map::Entry, error, fmt, io, num::NonZeroU64};

lazy_static! {
    static ref HUB: Mutex<FnvHashMap<NonZeroU64, mpsc::UnboundedSender<Channel<Bytes>>>> = Mutex::new(FnvHashMap::default());
}

/// Transport that supports `/memory/N` multiaddresses.
#[derive(Debug, Copy, Clone, Default)]
pub struct MemoryTransport;

impl Transport for MemoryTransport {
    type Output = Channel<Bytes>;
    type Error = MemoryTransportError;
    type Listener = Listener;
    type ListenerUpgrade = FutureResult<Self::Output, Self::Error>;
    type Dial = FutureResult<Self::Output, Self::Error>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>> {
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

        let actual_addr = Protocol::Memory(port.get()).into();

        let (tx, rx) = mpsc::unbounded();
        match hub.entry(port) {
            Entry::Occupied(_) => return Err(TransportError::Other(MemoryTransportError::Unreachable)),
            Entry::Vacant(e) => e.insert(tx),
        };

        let listener = Listener {
            port,
            receiver: rx,
        };

        Ok((listener, actual_addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
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
        let chan = if let Some(tx) = hub.get(&port) {
            let (a_tx, a_rx) = mpsc::unbounded();
            let (b_tx, b_rx) = mpsc::unbounded();
            let a = RwStreamSink::new(Chan { incoming: a_rx, outgoing: b_tx });
            let b = RwStreamSink::new(Chan { incoming: b_rx, outgoing: a_tx });
            if tx.unbounded_send(b).is_err() {
                return Err(TransportError::Other(MemoryTransportError::Unreachable));
            }
            a
        } else {
            return Err(TransportError::Other(MemoryTransportError::Unreachable));
        };

        Ok(future::ok(chan))
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        // TODO: NAT traversal for `/memory` addresses? how does that make sense?
        if server == observed {
            Some(server.clone())
        } else {
            None
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
    /// Receives incoming connections.
    receiver: mpsc::UnboundedReceiver<Channel<Bytes>>,
}

impl Stream for Listener {
    type Item = (FutureResult<Channel<Bytes>, MemoryTransportError>, Multiaddr);
    type Error = MemoryTransportError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let channel = try_ready!(Ok(self.receiver.poll()
            .expect("An unbounded receiver never panics; QED")));
        let channel = match channel {
            Some(c) => c,
            None => return Ok(Async::Ready(None)),
        };
        let dialed_addr = Protocol::Memory(self.port.get()).into();
        Ok(Async::Ready(Some((future::ok(channel), dialed_addr))))
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
    incoming: mpsc::UnboundedReceiver<T>,
    outgoing: mpsc::UnboundedSender<T>,
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
        assert!(transport.listen_on("/memory/5".parse().unwrap()).is_ok());
        assert!(transport.listen_on("/memory/5".parse().unwrap()).is_ok());
        let _listener = transport.listen_on("/memory/5".parse().unwrap()).unwrap();
        assert!(transport.listen_on("/memory/5".parse().unwrap()).is_err());
        assert!(transport.listen_on("/memory/5".parse().unwrap()).is_err());
        drop(_listener);
        assert!(transport.listen_on("/memory/5".parse().unwrap()).is_ok());
        assert!(transport.listen_on("/memory/5".parse().unwrap()).is_ok());
    }

    #[test]
    fn port_not_in_use() {
        let transport = MemoryTransport::default();
        assert!(transport.dial("/memory/5".parse().unwrap()).is_err());
        let _listener = transport.listen_on("/memory/5".parse().unwrap()).unwrap();
        assert!(transport.dial("/memory/5".parse().unwrap()).is_ok());
    }

    // TODO: test that is actually works
}
