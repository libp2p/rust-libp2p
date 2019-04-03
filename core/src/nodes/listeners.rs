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

use crate::{Id, Multiaddr, Transport, transport::{TransportError, ListenerEvent}};
use futures::prelude::*;
use std::{collections::{HashSet, VecDeque}, fmt};
use void::Void;

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
/// # fn main() {
/// use futures::prelude::*;
/// use libp2p_core::nodes::listeners::{ListenersEvent, ListenersStream};
///
/// let mut listeners = ListenersStream::new(libp2p_tcp::TcpConfig::new());
///
/// // Ask the `listeners` to start listening on the given multiaddress.
/// listeners.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap()).unwrap();
///
/// // You can retrieve the list of active listeners with `listeners()`.
/// println!("Listeners {:?}", listeners.listener_ids().collect::<Vec<_>>());
///
/// // The `listeners` will now generate events when polled.
/// let future = listeners.for_each(move |event| {
///     match event {
///         ListenersEvent::NewAddress { listener_id, listen_addr } => {
///             println!("Listener {} is listening at address {}", listener_id, listen_addr);
///         },
///         ListenersEvent::AddressExpired { listener_id, listen_addr } => {
///             println!("Listener {} is no longer listening at address {}", listener_id, listen_addr);
///         },
///         ListenersEvent::Closed { listener_id, listener, result } => {
///             println!("Listener {} has been closed: {:?}", listener_id, result);
///         },
///         ListenersEvent::Incoming { upgrade, listen_addr, .. } => {
///             println!("A connection has arrived on {}", listen_addr);
///             // We don't do anything with the newly-opened connection, but in a real-life
///             // program you probably want to use it!
///             drop(upgrade);
///         },
///     };
///
///     Ok(())
/// });
///
/// tokio::run(future.map_err(|_| ()));
/// # }
/// ```
pub struct ListenersStream<TTrans>
where
    TTrans: Transport,
{
    /// Transport used to spawn listeners.
    transport: TTrans,
    /// All the active listeners.
    listeners: VecDeque<Listener<TTrans>>,
    /// Counter of IDs assigned to listeners.
    id_counter: usize
}

/// A single active listener.
#[derive(Debug)]
struct Listener<TTrans>
where
    TTrans: Transport,
{
    /// The unique ID of this listener.
    id: Id,
    /// The object that actually listens.
    listener: TTrans::Listener,
    /// Addresses it is listening on.
    addresses: HashSet<Multiaddr>
}

/// Event that can happen on the `ListenersStream`.
pub enum ListenersEvent<TTrans>
where
    TTrans: Transport,
{
    /// A new address is being listened on.
    NewAddress {
        /// Listener listening on a new address.
        listener_id: Id,
        /// The new address that is being listened on.
        listen_addr: Multiaddr
    },
    /// An address is no longer being listened on.
    AddressExpired {
        /// Listener listening on a new address.
        listener_id: Id,
        /// The new address that is being listened on.
        listen_addr: Multiaddr
    },
    /// A connection is incoming on one of the listeners.
    Incoming {
        /// The produced upgrade.
        upgrade: TTrans::ListenerUpgrade,
        /// The ID of the listener producing this upgrade.
        listener_id: Id,
        /// Address of the listener which received the connection.
        listen_addr: Multiaddr,
        /// Address used to send back data to the incoming client.
        send_back_addr: Multiaddr,
    },

    /// A listener has closed, either gracefully or with an error.
    Closed {
        /// The ID of the listener that closed.
        listener_id: Id,
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
            listeners: VecDeque::new(),
            id_counter: 0
        }
    }

    /// Same as `new`, but pre-allocates enough memory for the given number of
    /// simultaneous listeners.
    #[inline]
    pub fn with_capacity(transport: TTrans, capacity: usize) -> Self {
        ListenersStream {
            transport,
            listeners: VecDeque::with_capacity(capacity),
            id_counter: 0,
        }
    }

    /// Start listening on a multiaddress.
    ///
    /// Returns an error if the transport doesn't support the given multiaddress.
    pub fn listen_on(&mut self, addr: Multiaddr) -> Result<Id, TransportError<TTrans::Error>>
    where
        TTrans: Clone,
    {
        let listener = self.transport.clone().listen_on(addr)?;
        let listener_id = Id(self.id_counter);

        self.listeners.push_back(Listener {
            listener,
            id: listener_id.clone(),
            addresses: HashSet::new()
        });

        self.id_counter += 1;

        Ok(listener_id)
    }

    /// Returns the transport passed when building this object.
    pub fn transport(&self) -> &TTrans {
        &self.transport
    }

    /// Returns and iterator over the [`Id`]s of all our listeners.
    pub fn listener_ids<'a>(&'a self) -> impl Iterator<Item = Id> + 'a {
        self.listeners.iter().map(|l| l.id.clone())
    }

    /// Returns an iterator that produces the list of addresses we're listening on.
    pub fn listen_addrs(&self) -> impl Iterator<Item = &Multiaddr> {
        self.listeners.iter().flat_map(|l| l.addresses.iter())
    }

    /// Provides an API similar to `Stream`, except that it cannot error.
    pub fn poll(&mut self) -> Async<ListenersEvent<TTrans>> {
        // We remove each element from `listeners` one by one and add them back.
        let mut remaining = self.listeners.len();
        while let Some(mut listener) = self.listeners.pop_back() {
            match listener.listener.poll() {
                Ok(Async::NotReady) => {
                    self.listeners.push_front(listener);
                    remaining -= 1;
                    if remaining == 0 { break }
                }
                Ok(Async::Ready(Some(ListenerEvent::Upgrade { upgrade, listen_addr, remote_addr }))) => {
                    debug_assert!(listener.addresses.contains(&listen_addr));
                    let id = listener.id.clone();
                    self.listeners.push_front(listener);
                    return Async::Ready(ListenersEvent::Incoming {
                        upgrade,
                        listener_id: id,
                        listen_addr,
                        send_back_addr: remote_addr
                    });
                }
                Ok(Async::Ready(Some(ListenerEvent::NewAddress(a)))) => {
                    listener.addresses.insert(a.clone());
                    let id = listener.id.clone();
                    self.listeners.push_front(listener);
                    return Async::Ready(ListenersEvent::NewAddress {
                        listener_id: id,
                        listen_addr: a
                    });
                }
                Ok(Async::Ready(Some(ListenerEvent::AddressExpired(a)))) => {
                    listener.addresses.remove(&a);
                    let id = listener.id.clone();
                    self.listeners.push_front(listener);
                    return Async::Ready(ListenersEvent::AddressExpired {
                        listener_id: id,
                        listen_addr: a
                    });
                }
                Ok(Async::Ready(None)) => {
                    return Async::Ready(ListenersEvent::Closed {
                        listener_id: listener.id,
                        listener: listener.listener,
                        result: Ok(()),
                    });
                }
                Err(err) => {
                    return Async::Ready(ListenersEvent::Closed {
                        listener_id: listener.id,
                        listener: listener.listener,
                        result: Err(err),
                    });
                }
            }
        }

        // We register the current task to be woken up if a new listener is added.
        Async::NotReady
    }
}

impl<TTrans> Stream for ListenersStream<TTrans>
where
    TTrans: Transport,
{
    type Item = ListenersEvent<TTrans>;
    type Error = Void; // TODO: use ! once stable

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        Ok(self.poll().map(Option::Some))
    }
}

impl<TTrans> fmt::Debug for ListenersStream<TTrans>
where
    TTrans: Transport + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.debug_struct("ListenersStream")
            .field("transport", &self.transport)
            .field("listener_ids", &self.listener_ids().collect::<Vec<_>>())
            .field("listen_addrs", &self.listen_addrs().collect::<Vec<_>>())
            .finish()
    }
}

impl<TTrans> fmt::Debug for ListenersEvent<TTrans>
where
    TTrans: Transport,
    <TTrans::Listener as Stream>::Error: fmt::Debug,
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
            ListenersEvent::Incoming { listener_id, listen_addr, .. } => f
                .debug_struct("ListenersEvent::Incoming")
                .field("listener_id", listener_id)
                .field("listen_addr", listen_addr)
                .finish(),
            ListenersEvent::Closed { listener_id, result, .. } => f
                .debug_struct("ListenersEvent::Closed")
                .field("listener_id", listener_id)
                .field("result", result)
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{self, ListenerEvent};
    use assert_matches::assert_matches;
    use tokio::runtime::current_thread::Runtime;
    use std::{io, iter::FromIterator};
    use futures::{future::{self}, stream};
    use crate::tests::dummy_transport::{DummyTransport, ListenerState};
    use crate::tests::dummy_muxer::DummyMuxer;
    use crate::PeerId;

    fn set_listener_state(ls: &mut ListenersStream<DummyTransport>, idx: usize, state: ListenerState) {
        ls.listeners[idx].listener = match state {
            ListenerState::Error =>
                Box::new(stream::poll_fn(|| Err(io::Error::new(io::ErrorKind::Other, "oh noes")))),
            ListenerState::Ok(state) => match state {
                Async::NotReady => Box::new(stream::poll_fn(|| Ok(Async::NotReady))),
                Async::Ready(Some(event)) => Box::new(stream::poll_fn(move || {
                    Ok(Async::Ready(Some(event.clone().map(future::ok))))
                })),
                Async::Ready(None) => Box::new(stream::empty())
            }
            ListenerState::Events(events) =>
                Box::new(stream::iter_ok(events.into_iter().map(|e| e.map(future::ok))))
        };
    }

    #[test]
    fn incoming_event() {
        let mem_transport = transport::MemoryTransport::default();

        let mut listeners = ListenersStream::new(mem_transport);
        listeners.listen_on("/memory/0".parse().unwrap()).unwrap();

        let address =
            if let Async::Ready(ListenersEvent::NewAddress { listen_addr, .. }) = listeners.poll() {
                listen_addr
            } else {
                panic!("Was expecting the listen address to be reported")
            };

        let dial = mem_transport.dial(address.clone()).unwrap();

        let future = listeners
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(event, _)| {
                match event {
                    Some(ListenersEvent::Incoming { listen_addr, upgrade, send_back_addr, .. }) => {
                        assert_eq!(listen_addr, address);
                        assert_eq!(send_back_addr, address);
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

    #[test]
    fn listener_stream_returns_transport() {
        let t = DummyTransport::new();
        let t_clone = t.clone();
        let ls = ListenersStream::new(t);
        assert_eq!(ls.transport(), &t_clone);
    }

    #[test]
    fn listener_stream_can_iterate_over_listeners() {
        let mut t = DummyTransport::new();
        let addr1 = tcp4([127, 0, 0, 1], 1234);
        let addr2 = tcp4([127, 0, 0, 1], 4321);

        t.set_initial_listener_state(ListenerState::Events(vec![
            ListenerEvent::NewAddress(addr1.clone()),
            ListenerEvent::NewAddress(addr2.clone())
        ]));

        let mut ls = ListenersStream::new(t);
        ls.listen_on(tcp4([0, 0, 0, 0], 0)).expect("listen_on");

        assert_matches!(ls.poll(), Async::Ready(ListenersEvent::NewAddress { listen_addr, .. }) => {
            assert_eq!(addr1, listen_addr)
        });
        assert_matches!(ls.poll(), Async::Ready(ListenersEvent::NewAddress { listen_addr, .. }) => {
            assert_eq!(addr2, listen_addr)
        })
    }

    #[test]
    fn listener_stream_poll_without_listeners_is_not_ready() {
        let t = DummyTransport::new();
        let mut ls = ListenersStream::new(t);
        assert_matches!(ls.poll(), Async::NotReady);
    }

    #[test]
    fn listener_stream_poll_with_listeners_that_arent_ready_is_not_ready() {
        let t = DummyTransport::new();
        let addr = tcp4([127, 0, 0, 1], 1234);
        let mut ls = ListenersStream::new(t);
        ls.listen_on(addr).expect("listen_on failed");
        set_listener_state(&mut ls, 0, ListenerState::Ok(Async::NotReady));
        assert_matches!(ls.poll(), Async::NotReady);
        assert_eq!(ls.listeners.len(), 1); // listener is still there
    }

    #[test]
    fn listener_stream_poll_with_ready_listeners_is_ready() {
        let mut t = DummyTransport::new();
        let peer_id = PeerId::random();
        let muxer = DummyMuxer::new();
        let expected_output = (peer_id.clone(), muxer.clone());

        t.set_initial_listener_state(ListenerState::Events(vec![
            ListenerEvent::NewAddress(tcp4([127, 0, 0, 1], 9090)),
            ListenerEvent::Upgrade {
                upgrade: (peer_id.clone(), muxer.clone()),
                listen_addr: tcp4([127, 0, 0, 1], 9090),
                remote_addr: tcp4([127, 0, 0, 1], 32000)
            },
            ListenerEvent::Upgrade {
                upgrade: (peer_id.clone(), muxer.clone()),
                listen_addr: tcp4([127, 0, 0, 1], 9090),
                remote_addr: tcp4([127, 0, 0, 1], 32000)
            },
            ListenerEvent::Upgrade {
                upgrade: (peer_id.clone(), muxer.clone()),
                listen_addr: tcp4([127, 0, 0, 1], 9090),
                remote_addr: tcp4([127, 0, 0, 1], 32000)
            }
        ]));

        let mut ls = ListenersStream::new(t);
        ls.listen_on(tcp4([127, 0, 0, 1], 1234)).expect("listen_on");
        ls.listen_on(tcp4([127, 0, 0, 1], 4321)).expect("listen_on");
        assert_eq!(ls.listeners.len(), 2);

        assert_matches!(ls.poll(), Async::Ready(listeners_event) => {
            assert_matches!(listeners_event, ListenersEvent::NewAddress { listener_id, .. } => {
                assert_eq!(1, listener_id.as_usize())
            })
        });

        assert_matches!(ls.poll(), Async::Ready(listeners_event) => {
            assert_matches!(listeners_event, ListenersEvent::NewAddress { listener_id, .. } => {
                assert_eq!(0, listener_id.as_usize())
            })
        });

        assert_matches!(ls.poll(), Async::Ready(listeners_event) => {
            assert_matches!(listeners_event, ListenersEvent::Incoming { mut upgrade, listener_id, .. } => {
                assert_eq!(1, listener_id.as_usize());
                assert_matches!(upgrade.poll().unwrap(), Async::Ready(output) => {
                    assert_eq!(output, expected_output)
                });
            })
        });

        assert_matches!(ls.poll(), Async::Ready(listeners_event) => {
            assert_matches!(listeners_event, ListenersEvent::Incoming { mut upgrade, listener_id, .. } => {
                assert_eq!(0, listener_id.as_usize());
                assert_matches!(upgrade.poll().unwrap(), Async::Ready(output) => {
                    assert_eq!(output, expected_output)
                });
            })
        });

        set_listener_state(&mut ls, 1, ListenerState::Ok(Async::NotReady));

        assert_matches!(ls.poll(), Async::Ready(listeners_event) => {
            assert_matches!(listeners_event, ListenersEvent::Incoming { mut upgrade, listener_id, .. } => {
                assert_eq!(0, listener_id.as_usize());
                assert_matches!(upgrade.poll().unwrap(), Async::Ready(output) => {
                    assert_eq!(output, expected_output)
                });
            })
        });
    }

    #[test]
    fn listener_stream_poll_with_closed_listener_emits_closed_event() {
        let t = DummyTransport::new();
        let addr = tcp4([127, 0, 0, 1], 1234);
        let mut ls = ListenersStream::new(t);
        ls.listen_on(addr).expect("listen_on failed");
        set_listener_state(&mut ls, 0, ListenerState::Ok(Async::Ready(None)));
        assert_matches!(ls.poll(), Async::Ready(listeners_event) => {
            assert_matches!(listeners_event, ListenersEvent::Closed{..})
        });
        assert_eq!(ls.listeners.len(), 0); // it's gone
    }

    #[test]
    fn listener_stream_poll_with_erroring_listener_emits_closed_event() {
        let mut t = DummyTransport::new();
        let peer_id = PeerId::random();
        let muxer = DummyMuxer::new();
        let event = ListenerEvent::Upgrade {
            upgrade: (peer_id, muxer),
            listen_addr: tcp4([127, 0, 0, 1], 1234),
            remote_addr: tcp4([127, 0, 0, 1], 32000)
        };
        t.set_initial_listener_state(ListenerState::Ok(Async::Ready(Some(event))));
        let addr = tcp4([127, 0, 0, 1], 1234);
        let mut ls = ListenersStream::new(t);
        ls.listen_on(addr).expect("listen_on failed");
        set_listener_state(&mut ls, 0, ListenerState::Error); // simulate an error on the socket
        assert_matches!(ls.poll(), Async::Ready(listeners_event) => {
            assert_matches!(listeners_event, ListenersEvent::Closed{..})
        });
        assert_eq!(ls.listeners.len(), 0); // it's gone
    }

    #[test]
    fn listener_stream_poll_chatty_listeners_each_get_their_turn() {
        let mut t = DummyTransport::new();
        let peer_id = PeerId::random();
        let muxer = DummyMuxer::new();

        t.set_initial_listener_state(ListenerState::Events(vec![
            ListenerEvent::NewAddress(tcp4([127, 0, 0, 1], 9090)),
            ListenerEvent::Upgrade {
                upgrade: (peer_id.clone(), muxer.clone()),
                listen_addr: tcp4([127, 0, 0, 1], 9090),
                remote_addr: tcp4([127, 0, 0, 1], 32000)
            },
            ListenerEvent::Upgrade {
                upgrade: (peer_id.clone(), muxer.clone()),
                listen_addr: tcp4([127, 0, 0, 1], 9090),
                remote_addr: tcp4([127, 0, 0, 1], 32000)
            },
            ListenerEvent::Upgrade {
                upgrade: (peer_id.clone(), muxer.clone()),
                listen_addr: tcp4([127, 0, 0, 1], 9090),
                remote_addr: tcp4([127, 0, 0, 1], 32000)
            },
            ListenerEvent::Upgrade {
                upgrade: (peer_id.clone(), muxer.clone()),
                listen_addr: tcp4([127, 0, 0, 1], 9090),
                remote_addr: tcp4([127, 0, 0, 1], 32000)
            },
            ListenerEvent::Upgrade {
                upgrade: (peer_id.clone(), muxer.clone()),
                listen_addr: tcp4([127, 0, 0, 1], 9090),
                remote_addr: tcp4([127, 0, 0, 1], 32000)
            }
        ]));

        let mut ls = ListenersStream::new(t);

        // Create 4 Listeners
        for _ in 0..4 {
            ls.listen_on(tcp4([127, 0, 0, 1], 9090)).expect("listen_on");
        }

        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::NewAddress { listener_id, listen_addr }) => {
                assert_eq!(n, listener_id.as_usize());
                assert_eq!(tcp4([127, 0, 0, 1], 9090), listen_addr)
            })
        }

        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming { listener_id, .. }) => {
                assert_eq!(n, listener_id.as_usize())
            })
        }

        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming { listener_id, .. }) => {
                assert_eq!(n, listener_id.as_usize())
            })
        }

        // Make last listener NotReady; it will become the first element and
        // retried after trying the other Listeners.
        set_listener_state(&mut ls, 3, ListenerState::Ok(Async::NotReady));

        for n in (0..3).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming { listener_id, .. }) => {
                assert_eq!(n, listener_id.as_usize())
            })
        }

        for n in (0..3).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming { listener_id, .. }) => {
                assert_eq!(n, listener_id.as_usize())
            })
        }

        // Turning the last listener back on means we now have 4 "good"
        // listeners, and each get their turn.
        let event = ListenerEvent::Upgrade {
            upgrade: (peer_id, DummyMuxer::new()),
            listen_addr: tcp4([127, 0, 0, 1], 9090),
            remote_addr: tcp4([127, 0, 0, 1], 32000)
        };
        set_listener_state(&mut ls, 3, ListenerState::Ok(Async::Ready(Some(event))));
        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming { listener_id, .. }) => {
                assert_eq!(n, listener_id.as_usize())
            })
        }
    }

    #[test]
    fn listener_stream_poll_processes_listeners_in_turn() {
        let mut t = DummyTransport::new();
        let peer_id = PeerId::random();
        let muxer = DummyMuxer::new();

        t.set_initial_listener_state(ListenerState::Events(vec![
            ListenerEvent::NewAddress(tcp4([127, 0, 0, 1], 9090)),
            ListenerEvent::Upgrade {
                upgrade: (peer_id.clone(), muxer.clone()),
                listen_addr: tcp4([127, 0, 0, 1], 9090),
                remote_addr: tcp4([127, 0, 0, 1], 32000)
            }
        ]));

        let mut ls = ListenersStream::new(t);

        for n in 0..4 {
            ls.listen_on(tcp4([127, 0, 0, n], u16::from(n))).expect("listen_on");
        }

        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::NewAddress { listener_id, listen_addr }) => {
                assert_eq!(n, listener_id.as_usize());
                assert_eq!(tcp4([127, 0, 0, 1], 9090), listen_addr)
            })
        }

        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming { listener_id, .. }) => {
                assert_eq!(n, listener_id.as_usize())
            });
            set_listener_state(&mut ls, 0, ListenerState::Ok(Async::NotReady));
        }
        // All Listeners are NotReady, so poll yields NotReady
        assert_matches!(ls.poll(), Async::NotReady);
    }

    fn tcp4(ip: [u8; 4], port: u16) -> Multiaddr {
        let protos = std::iter::once(multiaddr::Protocol::Ip4(ip.into()))
            .chain(std::iter::once(multiaddr::Protocol::Tcp(port)));
        Multiaddr::from_iter(protos)
    }
}
