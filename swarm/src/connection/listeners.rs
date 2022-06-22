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

use crate::{
    transport::{ListenerEvent, TransportError},
    Multiaddr, Transport,
};
use futures::{prelude::*, task::Context, task::Poll};
use libp2p_core::connection::ListenerId;
use log::debug;
use smallvec::SmallVec;
use std::{collections::VecDeque, fmt, mem, pin::Pin};

/// Implementation of `futures::Stream` that allows listening on multiaddresses.
///
/// To start using a [`ListenersStream`], create one with [`ListenersStream::new`] by passing an
/// implementation of [`Transport`]. This [`Transport`] will be used to start listening, therefore
/// you want to pass a [`Transport`] that supports the protocols you wish you listen on.
///
/// Then, call [`ListenersStream::listen_on`] for all addresses you want to start listening on.
///
/// The [`ListenersStream`] never ends and never produces errors. If a listener errors or closes, an
/// event is generated on the stream and the listener is then dropped, but the [`ListenersStream`]
/// itself continues.
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
    next_id: ListenerId,
    /// Pending listeners events to return from [`ListenersStream::poll`].
    pending_events: VecDeque<ListenersEvent<TTrans>>,
}

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
    addresses: SmallVec<[Multiaddr; 4]>,
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
        listen_addr: Multiaddr,
    },
    /// An address is no longer being listened on.
    AddressExpired {
        /// The listener that is no longer listening on the address.
        listener_id: ListenerId,
        /// The new address that is being listened on.
        listen_addr: Multiaddr,
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
    },
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
            next_id: ListenerId::new(1),
            pending_events: VecDeque::new(),
        }
    }

    /// Start listening on a multiaddress.
    ///
    /// Returns an error if the transport doesn't support the given multiaddress.
    pub fn listen_on(
        &mut self,
        addr: Multiaddr,
    ) -> Result<ListenerId, TransportError<TTrans::Error>> {
        let listener = self.transport.listen_on(addr)?;
        self.listeners.push_back(Box::pin(Listener {
            id: self.next_id,
            listener,
            addresses: SmallVec::new(),
        }));
        let id = self.next_id;
        self.next_id = self.next_id + 1;
        Ok(id)
    }

    /// Remove the listener matching the given `ListenerId`.
    ///
    /// Returns `true` if there was a listener with this ID, `false`
    /// otherwise.
    pub fn remove_listener(&mut self, id: ListenerId) -> bool {
        if let Some(i) = self.listeners.iter().position(|l| l.id == id) {
            let mut listener = self
                .listeners
                .remove(i)
                .expect("Index can not be out of bounds.");
            let listener_project = listener.as_mut().project();
            let addresses = mem::take(listener_project.addresses).into_vec();
            self.pending_events.push_back(ListenersEvent::Closed {
                listener_id: *listener_project.id,
                addresses,
                reason: Ok(()),
            });
            true
        } else {
            false
        }
    }

    /// Returns a reference to the transport passed when building this object.
    pub fn transport(&self) -> &TTrans {
        &self.transport
    }

    /// Returns a mutable reference to the transport passed when building this object.
    pub fn transport_mut(&mut self) -> &mut TTrans {
        &mut self.transport
    }

    /// Returns an iterator that produces the list of addresses we're listening on.
    pub fn listen_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listeners.iter().flat_map(|l| l.addresses.iter())
    }

    /// Provides an API similar to `Stream`, except that it cannot end.
    pub fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<ListenersEvent<TTrans>> {
        // Return pending events from closed listeners.
        if let Some(event) = self.pending_events.pop_front() {
            return Poll::Ready(event);
        }
        // We remove each element from `listeners` one by one and add them back.
        let mut remaining = self.listeners.len();
        while let Some(mut listener) = self.listeners.pop_back() {
            let mut listener_project = listener.as_mut().project();
            match TryStream::try_poll_next(listener_project.listener.as_mut(), cx) {
                Poll::Pending => {
                    self.listeners.push_front(listener);
                    remaining -= 1;
                    if remaining == 0 {
                        break;
                    }
                }
                Poll::Ready(Some(Ok(ListenerEvent::Upgrade {
                    upgrade,
                    local_addr,
                    remote_addr,
                }))) => {
                    let id = *listener_project.id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(ListenersEvent::Incoming {
                        listener_id: id,
                        upgrade,
                        local_addr,
                        send_back_addr: remote_addr,
                    });
                }
                Poll::Ready(Some(Ok(ListenerEvent::NewAddress(a)))) => {
                    if listener_project.addresses.contains(&a) {
                        debug!("Transport has reported address {} multiple times", a)
                    } else {
                        listener_project.addresses.push(a.clone());
                    }
                    let id = *listener_project.id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(ListenersEvent::NewAddress {
                        listener_id: id,
                        listen_addr: a,
                    });
                }
                Poll::Ready(Some(Ok(ListenerEvent::AddressExpired(a)))) => {
                    listener_project.addresses.retain(|x| x != &a);
                    let id = *listener_project.id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(ListenersEvent::AddressExpired {
                        listener_id: id,
                        listen_addr: a,
                    });
                }
                Poll::Ready(Some(Ok(ListenerEvent::Error(error)))) => {
                    let id = *listener_project.id;
                    self.listeners.push_front(listener);
                    return Poll::Ready(ListenersEvent::Error {
                        listener_id: id,
                        error,
                    });
                }
                Poll::Ready(None) => {
                    let addresses = mem::take(listener_project.addresses).into_vec();
                    return Poll::Ready(ListenersEvent::Closed {
                        listener_id: *listener_project.id,
                        addresses,
                        reason: Ok(()),
                    });
                }
                Poll::Ready(Some(Err(err))) => {
                    let addresses = mem::take(listener_project.addresses).into_vec();
                    return Poll::Ready(ListenersEvent::Closed {
                        listener_id: *listener_project.id,
                        addresses,
                        reason: Err(err),
                    });
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

impl<TTrans> Unpin for ListenersStream<TTrans> where TTrans: Transport {}

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
            ListenersEvent::NewAddress {
                listener_id,
                listen_addr,
            } => f
                .debug_struct("ListenersEvent::NewAddress")
                .field("listener_id", listener_id)
                .field("listen_addr", listen_addr)
                .finish(),
            ListenersEvent::AddressExpired {
                listener_id,
                listen_addr,
            } => f
                .debug_struct("ListenersEvent::AddressExpired")
                .field("listener_id", listener_id)
                .field("listen_addr", listen_addr)
                .finish(),
            ListenersEvent::Incoming {
                listener_id,
                local_addr,
                ..
            } => f
                .debug_struct("ListenersEvent::Incoming")
                .field("listener_id", listener_id)
                .field("local_addr", local_addr)
                .finish(),
            ListenersEvent::Closed {
                listener_id,
                addresses,
                reason,
            } => f
                .debug_struct("ListenersEvent::Closed")
                .field("listener_id", listener_id)
                .field("addresses", addresses)
                .field("reason", reason)
                .finish(),
            ListenersEvent::Error { listener_id, error } => f
                .debug_struct("ListenersEvent::Error")
                .field("listener_id", listener_id)
                .field("error", error)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::{future::BoxFuture, stream::BoxStream};

    use super::*;
    use crate::transport;

    #[test]
    fn incoming_event() {
        async_std::task::block_on(async move {
            let mut mem_transport = transport::MemoryTransport::default();

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
                ListenersEvent::Incoming {
                    local_addr,
                    send_back_addr,
                    ..
                } => {
                    assert_eq!(local_addr, address);
                    assert!(send_back_addr != address);
                }
                _ => panic!(),
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
            type Listener = BoxStream<
                'static,
                Result<ListenerEvent<Self::ListenerUpgrade, std::io::Error>, std::io::Error>,
            >;
            type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
            type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

            fn listen_on(
                &mut self,
                _: Multiaddr,
            ) -> Result<Self::Listener, transport::TransportError<Self::Error>> {
                Ok(Box::pin(stream::unfold((), |()| async move {
                    Some((
                        Ok(ListenerEvent::Error(std::io::Error::from(
                            std::io::ErrorKind::Other,
                        ))),
                        (),
                    ))
                })))
            }

            fn dial(
                &mut self,
                _: Multiaddr,
            ) -> Result<Self::Dial, transport::TransportError<Self::Error>> {
                panic!()
            }

            fn dial_as_listener(
                &mut self,
                _: Multiaddr,
            ) -> Result<Self::Dial, transport::TransportError<Self::Error>> {
                panic!()
            }

            fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
                None
            }
        }

        async_std::task::block_on(async move {
            let transport = DummyTrans;
            let mut listeners = ListenersStream::new(transport);
            listeners.listen_on("/memory/0".parse().unwrap()).unwrap();

            for _ in 0..10 {
                match listeners.next().await.unwrap() {
                    ListenersEvent::Error { .. } => {}
                    _ => panic!(),
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
            type Listener = BoxStream<
                'static,
                Result<ListenerEvent<Self::ListenerUpgrade, std::io::Error>, std::io::Error>,
            >;
            type ListenerUpgrade = BoxFuture<'static, Result<Self::Output, Self::Error>>;
            type Dial = BoxFuture<'static, Result<Self::Output, Self::Error>>;

            fn listen_on(
                &mut self,
                _: Multiaddr,
            ) -> Result<Self::Listener, transport::TransportError<Self::Error>> {
                Ok(Box::pin(stream::unfold((), |()| async move {
                    Some((Err(std::io::Error::from(std::io::ErrorKind::Other)), ()))
                })))
            }

            fn dial(
                &mut self,
                _: Multiaddr,
            ) -> Result<Self::Dial, transport::TransportError<Self::Error>> {
                panic!()
            }

            fn dial_as_listener(
                &mut self,
                _: Multiaddr,
            ) -> Result<Self::Dial, transport::TransportError<Self::Error>> {
                panic!()
            }

            fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
                None
            }
        }

        async_std::task::block_on(async move {
            let transport = DummyTrans;
            let mut listeners = ListenersStream::new(transport);
            listeners.listen_on("/memory/0".parse().unwrap()).unwrap();

            match listeners.next().await.unwrap() {
                ListenersEvent::Closed { .. } => {}
                _ => panic!(),
            }
        });
    }

    #[test]
    fn listener_closed() {
        async_std::task::block_on(async move {
            let mem_transport = transport::MemoryTransport::default();

            let mut listeners = ListenersStream::new(mem_transport);
            let id = listeners.listen_on("/memory/0".parse().unwrap()).unwrap();

            let event = listeners.next().await.unwrap();
            let addr;
            if let ListenersEvent::NewAddress { listen_addr, .. } = event {
                addr = listen_addr
            } else {
                panic!("Was expecting the listen address to be reported")
            }

            assert!(listeners.remove_listener(id));

            match listeners.next().await.unwrap() {
                ListenersEvent::Closed {
                    listener_id,
                    addresses,
                    reason: Ok(()),
                } => {
                    assert_eq!(listener_id, id);
                    assert!(addresses.contains(&addr));
                }
                other => panic!("Unexpected listeners event: {:?}", other),
            }
        });
    }
}
