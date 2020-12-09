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

//! Manage listening on multiple multiaddresses at once.

use crate::{Multiaddr, Transport, transport::{TransportError, ListenerEvent}};
use futures::{prelude::*, task::Context, task::Poll};
use log::debug;
use smallvec::SmallVec;
use std::{collections::VecDeque, fmt, pin::Pin};

/// Implementation of `futures::Stream` that allows listening on multiaddresses.
///
/// To start using a `ListenersStream`, create one with `new` by passing an implementation of
/// `Transport`. This `Transport` will be used to start listening, therefore you want to pass
/// a `Transport` that supports the protocols you wish you listen on.
///
/// Then, call `ListenerStream::listen_on` for all addresses you want to start listening on.
///
/// The `ListenersStream` never ends and never produces errors. If a listener errors or closes,
/// an event is generated on the stream and the listener is then dropped, but the `ListenersStream`
/// itself continues.
///
/// # Example
///
/// ```no_run
/// use futures::prelude::*;
/// use libp2p_core::connection::{ListenersEvent, ListenersStream};
///
/// let mut listeners = ListenersStream::new(libp2p_tcp::TcpConfig::new());
///
/// // Ask the `listeners` to start listening on the given multiaddress.
/// listeners.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap()).unwrap();
///
/// // The `listeners` will now generate events when polled.
/// futures::executor::block_on(async move {
///     while let Some(event) = listeners.next().await {
///         match event {
///             ListenersEvent::NewAddress { listener_id, listen_addr } => {
///                 println!("Listener {:?} is listening at address {}", listener_id, listen_addr);
///             },
///             ListenersEvent::AddressExpired { listener_id, listen_addr } => {
///                 println!("Listener {:?} is no longer listening at address {}", listener_id, listen_addr);
///             },
///             ListenersEvent::Closed { listener_id, .. } => {
///                 println!("Listener {:?} has been closed", listener_id);
///             },
///             ListenersEvent::Error { listener_id, error } => {
///                 println!("Listener {:?} has experienced an error: {}", listener_id, error);
///             },
///             ListenersEvent::Incoming { listener_id, upgrade, local_addr, .. } => {
///                 println!("Listener {:?} has a new connection on {}", listener_id, local_addr);
///                 // We don't do anything with the newly-opened connection, but in a real-life
///                 // program you probably want to use it!
///                 drop(upgrade);
///             },
///         }
///     }
/// })
/// ```
pub struct ListenersStream<TTrans>
where
    TTrans: Transport,
{
    /// Transport used to spawn listeners.
    transport: TTrans,
    /// All the active listeners.
    /// The `Listener` struct contains a stream that we want to be pinned. Since the `VecDeque`
    /// can be resized, the only way is to use a `Pin<Box<>>`.
    listeners: VecDeque<Pin<Box<Listener<TTrans>>>>,
    /// The next listener ID to assign.
    next_id: ListenerId
}

/// The ID of a single listener.
///
/// It is part of most [`ListenersEvent`]s and can be used to remove
/// individual listeners from the [`ListenersStream`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ListenerId(u64);

/// A single active listener.
#[pin_project::pin_project]
#[derive(Debug)]
struct Listener<TTrans>
where
    TTrans: Transport,
{
    /// The ID of this listener.
    id: ListenerId,
    /// The object that actually listens.
    #[pin]
    listener: TTrans::Listener,
    /// Addresses it is listening on.
    addresses: SmallVec<[Multiaddr; 4]>
}

/// Event that can happen on the `ListenersStream`.
pub enum ListenersEvent<TTrans>
where
    TTrans: Transport,
{
    /// A new address is being listened on.
    NewAddress {
        /// The listener that is listening on the new address.
        listener_id: ListenerId,
        /// The new address that is being listened on.
        listen_addr: Multiaddr
    },
    /// An address is no longer being listened on.
    AddressExpired {
        /// The listener that is no longer listening on the address.
        listener_id: ListenerId,
        /// The new address that is being listened on.
        listen_addr: Multiaddr
    },
    /// A connection is incoming on one of the listeners.
    Incoming {
        /// The listener that produced the upgrade.
        listener_id: ListenerId,
        /// The produced upgrade.
        upgrade: TTrans::ListenerUpgrade,
        /// Local connection address.
        local_addr: Multiaddr,
        /// Address used to send back data to the incoming client.
        send_back_addr: Multiaddr,
    },
    /// A listener closed.
    Closed {
        /// The ID of the listener that closed.
        listener_id: ListenerId,
        /// The addresses that the listener was listening on.
        addresses: Vec<Multiaddr>,
        /// Reason for the closure. Contains `Ok(())` if the stream produced `None`, or `Err`
        /// if the stream produced an error.
        reason: Result<(), TTrans::Error>,
    },
    /// A listener errored.
    ///
    /// The listener will continue to be polled for new events and the event
    /// is for informational purposes only.
    Error {
        /// The ID of the listener that errored.
        listener_id: ListenerId,
        /// The error value.
        error: TTrans::Error,
    }
}

impl<TTrans> ListenersStream<TTrans>
where
    TTrans: Transport,
{
    /// Starts a new stream of listeners.
    pub fn new(transport: TTrans) -> Self {
        ListenersStream {
            transport,
            listeners: VecDeque::new(),
            next_id: ListenerId(1)
        }
    }

    /// Same as `new`, but pre-allocates enough memory for the given number of
    /// simultaneous listeners.
    pub fn with_capacity(transport: TTrans, capacity: usize) -> Self {
        ListenersStream {
            transport,
            listeners: VecDeque::with_capacity(capacity),
            next_id: ListenerId(1)
        }
    }

    /// Start listening on a multiaddress.
    ///
    /// Returns an error if the transport doesn't support the given multiaddress.
    pub fn listen_on(&mut self, addr: Multiaddr) -> Result<ListenerId, TransportError<TTrans::Error>>
    where
        TTrans: Clone,
    {
        let listener = self.transport.clone().listen_on(addr)?;
        self.listeners.push_back(Box::pin(Listener {
            id: self.next_id,
            listener,
            addresses: SmallVec::new()
        }));
        let id = self.next_id;
        self.next_id = ListenerId(self.next_id.0 + 1);
        Ok(id)
    }

    /// Remove the listener matching the given `ListenerId`.
    ///
    /// Return `Ok(())` if a listener with this ID was in the list.
    pub fn remove_listener(&mut self, id: ListenerId) -> Result<(), ()> {
        if let Some(i) = self.listeners.iter().position(|l| l.id == id) {
            self.listeners.remove(i);
            Ok(())
        } else {
            Err(())
        }
    }

    /// Returns the transport passed when building this object.
    pub fn transport(&self) -> &TTrans {
        &self.transport
    }

    /// Returns an iterator that produces the list of addresses we're listening on.
    pub fn listen_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listeners.iter().flat_map(|l| l.addresses.iter())
    }

    /// Provides an API similar to `Stream`, except that it cannot end.
    pub fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<ListenersEvent<TTrans>> {
        // We remove each element from `listeners` one by one and add them back.
        let mut remaining = self.listeners.len();
        while let Some(mut listener) = self.listeners.pop_back() {
            let mut listener_project = listener.as_mut().project();
            match TryStream::try_poll_next(listener_project.listener.as_mut(), cx) {
                Poll::Pending => {
                    self.listeners.push_front(listener);
                    remaining -= 1;
                    if remaining == 0 { break }
                }
                Poll::Ready(Some(Ok(ListenerEvent::Upgrade { upgrade, local_addr, remote_addr }))) => {
                    let id = *listener_project.id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(ListenersEvent::Incoming {
                        listener_id: id,
                        upgrade,
                        local_addr,
                        send_back_addr: remote_addr
                    })
                }
                Poll::Ready(Some(Ok(ListenerEvent::NewAddress(a)))) => {
                    if listener_project.addresses.contains(&a) {
                        debug!("Transport has reported address {} multiple times", a)
                    }
                    if !listener_project.addresses.contains(&a) {
                        listener_project.addresses.push(a.clone());
                    }
                    let id = *listener_project.id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(ListenersEvent::NewAddress {
                        listener_id: id,
                        listen_addr: a
                    })
                }
                Poll::Ready(Some(Ok(ListenerEvent::AddressExpired(a)))) => {
                    listener_project.addresses.retain(|x| x != &a);
                    let id = *listener_project.id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(ListenersEvent::AddressExpired {
                        listener_id: id,
                        listen_addr: a
                    })
                }
                Poll::Ready(Some(Ok(ListenerEvent::Error(error)))) => {
                    let id = *listener_project.id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(ListenersEvent::Error {
                        listener_id: id,
                        error,
                    })
                }
                Poll::Ready(None) => {
                    return Poll::Ready(ListenersEvent::Closed {
                        listener_id: *listener_project.id,
                        addresses: listener_project.addresses.drain(..).collect(),
                        reason: Ok(()),
                    })
                }
                Poll::Ready(Some(Err(err))) => {
                    return Poll::Ready(ListenersEvent::Closed {
                        listener_id: *listener_project.id,
                        addresses: listener_project.addresses.drain(..).collect(),
                        reason: Err(err),
                    })
                }
            }
        }

        // We register the current task to be woken up if a new listener is added.
        Poll::Pending
    }
}

impl<TTrans> Stream for ListenersStream<TTrans>
where
    TTrans: Transport,
{
    type Item = ListenersEvent<TTrans>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        ListenersStream::poll(self, cx).map(Option::Some)
    }
}

impl<TTrans> Unpin for ListenersStream<TTrans>
where
    TTrans: Transport,
{
}

impl<TTrans> fmt::Debug for ListenersStream<TTrans>
where
    TTrans: Transport + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("ListenersStream")
            .field("transport", &self.transport)
            .field("listen_addrs", &self.listen_addrs().collect::<Vec<_>>())
            .finish()
    }
}

impl<TTrans> fmt::Debug for ListenersEvent<TTrans>
where
    TTrans: Transport,
    TTrans::Error: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            ListenersEvent::NewAddress { listener_id, listen_addr } => f
                .debug_struct("ListenersEvent::NewAddress")
                .field("listener_id", listener_id)
                .field("listen_addr", listen_addr)
                .finish(),
            ListenersEvent::AddressExpired { listener_id, listen_addr } => f
                .debug_struct("ListenersEvent::AddressExpired")
                .field("listener_id", listener_id)
                .field("listen_addr", listen_addr)
                .finish(),
            ListenersEvent::Incoming { listener_id, local_addr, .. } => f
                .debug_struct("ListenersEvent::Incoming")
                .field("listener_id", listener_id)
                .field("local_addr", local_addr)
                .finish(),
            ListenersEvent::Closed { listener_id, addresses, reason } => f
                .debug_struct("ListenersEvent::Closed")
                .field("listener_id", listener_id)
                .field("addresses", addresses)
                .field("reason", reason)
                .finish(),
            ListenersEvent::Error { listener_id, error } => f
                .debug_struct("ListenersEvent::Error")
                .field("listener_id", listener_id)
                .field("error", error)
                .finish()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport;

    #[test]
    fn incoming_event() {
        async_std::task::block_on(async move {
            let mem_transport = transport::MemoryTransport::default();

            let mut listeners = ListenersStream::new(mem_transport);
            listeners.listen_on("/memory/0".parse().unwrap()).unwrap();

            let address = {
                let event = listeners.next().await.unwrap();
                if let ListenersEvent::NewAddress { listen_addr, .. } = event {
                    listen_addr
                } else {
                    panic!("Was expecting the listen address to be reported")
                }
            };

            let address2 = address.clone();
            async_std::task::spawn(async move {
                mem_transport.dial(address2).unwrap().await.unwrap();
            });

            match listeners.next().await.unwrap() {
                ListenersEvent::Incoming { local_addr, send_back_addr, .. } => {
                    assert_eq!(local_addr, address);
                    assert!(send_back_addr != address);
                },
                _ => panic!()
            }
        });
    }

    #[test]
    fn listener_event_error_isnt_fatal() {
        // Tests that a listener continues to be polled even after producing
        // a `ListenerEvent::Error`.

        #[derive(Clone)]
        struct DummyTrans;
        impl transport::Transport for DummyTrans {
            type Output = ();
            type Error = std::io::Error;
            type Listener = Pin<Box<dyn Stream<Item = Result<ListenerEvent<Self::ListenerUpgrade, std::io::Error>, std::io::Error>>>>;
            type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>>>>;
            type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>>>>;

            fn listen_on(self, _: Multiaddr) -> Result<Self::Listener, transport::TransportError<Self::Error>> {
                Ok(Box::pin(stream::unfold((), |()| async move {
                    Some((Ok(ListenerEvent::Error(std::io::Error::from(std::io::ErrorKind::Other))), ()))
                })))
            }

            fn dial(self, _: Multiaddr) -> Result<Self::Dial, transport::TransportError<Self::Error>> {
                panic!()
            }

            fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> { None }
        }

        async_std::task::block_on(async move {
            let transport = DummyTrans;
            let mut listeners = ListenersStream::new(transport);
            listeners.listen_on("/memory/0".parse().unwrap()).unwrap();

            for _ in 0..10 {
                match listeners.next().await.unwrap() {
                    ListenersEvent::Error { .. } => {},
                    _ => panic!()
                }
            }
        });
    }

    #[test]
    fn listener_error_is_fatal() {
        // Tests that a listener stops after producing an error on the stream itself.

        #[derive(Clone)]
        struct DummyTrans;
        impl transport::Transport for DummyTrans {
            type Output = ();
            type Error = std::io::Error;
            type Listener = Pin<Box<dyn Stream<Item = Result<ListenerEvent<Self::ListenerUpgrade, std::io::Error>, std::io::Error>>>>;
            type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>>>>;
            type Dial = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>>>>;

            fn listen_on(self, _: Multiaddr) -> Result<Self::Listener, transport::TransportError<Self::Error>> {
                Ok(Box::pin(stream::unfold((), |()| async move {
                    Some((Err(std::io::Error::from(std::io::ErrorKind::Other)), ()))
                })))
            }

            fn dial(self, _: Multiaddr) -> Result<Self::Dial, transport::TransportError<Self::Error>> {
                panic!()
            }

            fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> { None }
        }

        async_std::task::block_on(async move {
            let transport = DummyTrans;
            let mut listeners = ListenersStream::new(transport);
            listeners.listen_on("/memory/0".parse().unwrap()).unwrap();

            match listeners.next().await.unwrap() {
                ListenersEvent::Closed { .. } => {},
                _ => panic!()
            }
        });
    }
}
