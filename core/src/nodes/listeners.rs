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

use futures::{prelude::*, task};
use std::fmt;
use void::Void;
use {Multiaddr, Transport};

/// Implementation of `Stream` that handles listeners.
///
/// The stream cannot produce errors.
pub struct ListenersStream<TTrans>
where
    TTrans: Transport,
{
    /// Transport used to spawn listeners.
    transport: TTrans,
    /// All the active listeners.
    listeners: Vec<Listener<TTrans>>,
    /// Task to notify when we add a new listener to `listeners`, so that we start polling.
    to_notify: Option<task::Task>,
}

/// A single active listener.
struct Listener<TTrans>
where
    TTrans: Transport,
{
    /// The object that actually listens.
    listener: TTrans::Listener,
    /// Address it is listening on.
    address: Multiaddr,
}

/// Event that can happen on the `ListenersStream`.
pub enum ListenersEvent<TTrans>
where
    TTrans: Transport,
{
    /// A connection is incoming on one of the listeners.
    Incoming {
        /// The produced upgrade.
        upgrade: TTrans::ListenerUpgrade,
        /// Address of the listener which received the connection.
        listen_addr: Multiaddr,
    },

    /// A listener has closed, either gracefully or with an error.
    Closed {
        /// Address of the listener which closed.
        listen_addr: Multiaddr,
        /// The listener that closed.
        listener: TTrans::Listener,
        /// The error that happened. `Ok` if gracefully closed.
        result: Result<(), <TTrans::Listener as Stream>::Error>,
    },
}

impl<TTrans> ListenersStream<TTrans>
where
    TTrans: Transport,
{
    /// Starts a new stream of listeners.
    #[inline]
    pub fn new(transport: TTrans) -> Self {
        ListenersStream {
            transport,
            listeners: Vec::new(),
            to_notify: None,
        }
    }

    /// Same as `new`, but pre-allocates enough memory for the given number of
    /// simultaneous listeners.
    #[inline]
    pub fn with_capacity(transport: TTrans, capacity: usize) -> Self {
        ListenersStream {
            transport,
            listeners: Vec::with_capacity(capacity),
            to_notify: None,
        }
    }

    /// Start listening on a multiaddress.
    ///
    /// Returns an error if the transport doesn't support the given multiaddress.
    pub fn listen_on(&mut self, addr: Multiaddr) -> Result<Multiaddr, Multiaddr>
    where
        TTrans: Clone,
    {
        let (listener, new_addr) = self
            .transport
            .clone()
            .listen_on(addr)
            .map_err(|(_, addr)| addr)?;

        self.listeners.push(Listener {
            listener,
            address: new_addr.clone(),
        });

        if let Some(task) = self.to_notify.take() {
            task.notify();
        }

        Ok(new_addr)
    }

    /// Returns the transport passed when building this object.
    #[inline]
    pub fn transport(&self) -> &TTrans {
        &self.transport
    }

    /// Returns an iterator that produces the list of addresses we're listening on.
    #[inline]
    pub fn listeners(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listeners.iter().map(|l| &l.address)
    }
}

impl<TTrans> Stream for ListenersStream<TTrans>
where
    TTrans: Transport,
{
    type Item = ListenersEvent<TTrans>;
    type Error = Void; // TODO: use ! once stable

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // We remove each element from `listeners` one by one and add them back.
        for n in (0..self.listeners.len()).rev() {
            let mut listener = self.listeners.swap_remove(n);
            match listener.listener.poll() {
                Ok(Async::NotReady) => {
                    self.listeners.push(listener);
                }
                Ok(Async::Ready(Some(upgrade))) => {
                    let listen_addr = listener.address.clone();
                    self.listeners.push(listener);
                    return Ok(Async::Ready(Some(ListenersEvent::Incoming {
                        upgrade,
                        listen_addr,
                    })));
                }
                Ok(Async::Ready(None)) => {
                    return Ok(Async::Ready(Some(ListenersEvent::Closed {
                        listen_addr: listener.address,
                        listener: listener.listener,
                        result: Ok(()),
                    })));
                }
                Err(err) => {
                    return Ok(Async::Ready(Some(ListenersEvent::Closed {
                        listen_addr: listener.address,
                        listener: listener.listener,
                        result: Err(err),
                    })));
                }
            }
        }

        // We register the current task to be waken up if a new listener is added.
        self.to_notify = Some(task::current());
        Ok(Async::NotReady)
    }
}

impl<TTrans> fmt::Debug for ListenersStream<TTrans>
where
    TTrans: Transport + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        f.debug_struct("ListenersStream")
            .field("transport", &self.transport)
            .field("listeners", &self.listeners().collect::<Vec<_>>())
            .finish()
    }
}

impl<TTrans> fmt::Debug for ListenersEvent<TTrans>
where
    TTrans: Transport,
    <TTrans::Listener as Stream>::Error: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match self {
            ListenersEvent::Incoming {
                ref listen_addr, ..
            } => f
                .debug_struct("ListenersEvent::Incoming")
                .field("listen_addr", listen_addr)
                .finish(),
            ListenersEvent::Closed {
                ref listen_addr,
                ref result,
                ..
            } => f
                .debug_struct("ListenersEvent::Closed")
                .field("listen_addr", listen_addr)
                .field("result", result)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp_transport;
    use super::*;
    use transport;
    use tokio::runtime::current_thread::Runtime;

    #[test]
    fn incoming_event() {
        let (tx, rx) = transport::connector();

        let mut listeners = ListenersStream::new(rx);
        listeners.listen_on("/memory".parse().unwrap()).unwrap();

        let dial = tx.dial("/memory".parse().unwrap()).unwrap_or_else(|_| panic!());

        let future = listeners
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(event, _)| {
                match event {
                    Some(ListenersEvent::Incoming { listen_addr, upgrade }) => {
                        assert_eq!(listen_addr, "/memory".parse().unwrap());
                        upgrade.map(|_| ()).map_err(|_| panic!())
                    },
                    _ => panic!()
                }
            })
            .select(dial.map(|_| ()).map_err(|_| panic!()))
            .map_err(|(err, _)| err);

        let mut runtime = Runtime::new().unwrap();
        runtime.block_on(future).unwrap();
    }
}
