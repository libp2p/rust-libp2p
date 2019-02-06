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

use crate::{Multiaddr, Transport, transport::TransportError};
use futures::prelude::*;
use std::{collections::VecDeque, fmt};
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
/// println!("Listening on: {:?}", listeners.listeners().collect::<Vec<_>>());
///
/// // The `listeners` will now generate events when polled.
/// let future = listeners.for_each(move |event| {
///     match event {
///         ListenersEvent::Closed { listen_addr, listener, result } => {
///             println!("Listener {} has been closed: {:?}", listen_addr, result);
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
}

/// A single active listener.
#[derive(Debug)]
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
        /// Address used to send back data to the incoming client.
        send_back_addr: Multiaddr,
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
            listeners: VecDeque::new(),
        }
    }

    /// Same as `new`, but pre-allocates enough memory for the given number of
    /// simultaneous listeners.
    #[inline]
    pub fn with_capacity(transport: TTrans, capacity: usize) -> Self {
        ListenersStream {
            transport,
            listeners: VecDeque::with_capacity(capacity),
        }
    }

    /// Start listening on a multiaddress.
    ///
    /// Returns an error if the transport doesn't support the given multiaddress.
    pub fn listen_on(&mut self, addr: Multiaddr) -> Result<Multiaddr, TransportError<TTrans::Error>>
    where
        TTrans: Clone,
    {
        let (listener, new_addr) = self
            .transport
            .clone()
            .listen_on(addr)?;

        self.listeners.push_back(Listener {
            listener,
            address: new_addr.clone(),
        });

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
                Ok(Async::Ready(Some((upgrade, send_back_addr)))) => {
                    let listen_addr = listener.address.clone();
                    self.listeners.push_front(listener);
                    return Async::Ready(ListenersEvent::Incoming {
                        upgrade,
                        listen_addr,
                        send_back_addr,
                    });
                }
                Ok(Async::Ready(None)) => {
                    return Async::Ready(ListenersEvent::Closed {
                        listen_addr: listener.address,
                        listener: listener.listener,
                        result: Ok(()),
                    });
                }
                Err(err) => {
                    return Async::Ready(ListenersEvent::Closed {
                        listen_addr: listener.address,
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
            .field("listeners", &self.listeners().collect::<Vec<_>>())
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
    use super::*;
    use crate::transport;
    use assert_matches::assert_matches;
    use tokio::runtime::current_thread::Runtime;
    use std::io;
    use futures::{future::{self}, stream};
    use crate::tests::dummy_transport::{DummyTransport, ListenerState};
    use crate::tests::dummy_muxer::DummyMuxer;
    use crate::PeerId;

    fn set_listener_state(ls: &mut ListenersStream<DummyTransport>, idx: usize, state: ListenerState) {
        let l = &mut ls.listeners[idx];
        l.listener =
            match state {
                ListenerState::Error => {
                    let stream = stream::poll_fn(|| future::err(io::Error::new(io::ErrorKind::Other, "oh noes")).poll() );
                    Box::new(stream)
                }
                ListenerState::Ok(r#async) => {
                    match r#async {
                        Async::NotReady => {
                            let stream = stream::poll_fn(|| Ok(Async::NotReady));
                            Box::new(stream)
                        }
                        Async::Ready(Some(tup)) => {
                            let addr = l.address.clone();
                            let stream = stream::poll_fn(move || Ok( Async::Ready(Some(tup.clone())) ))
                                .map(move |stream| (future::ok(stream), addr.clone()));
                            Box::new(stream)
                        }
                        Async::Ready(None) => {
                            let stream = stream::empty();
                            Box::new(stream)
                        }
                    }
                }
            };
    }

    #[test]
    fn incoming_event() {
        let (tx, rx) = transport::connector();

        let mut listeners = ListenersStream::new(rx);
        listeners.listen_on("/memory".parse().unwrap()).unwrap();

        let dial = tx.dial("/memory".parse().unwrap()).unwrap();

        let future = listeners
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(event, _)| {
                match event {
                    Some(ListenersEvent::Incoming { listen_addr, upgrade, send_back_addr }) => {
                        assert_eq!(listen_addr, "/memory".parse().unwrap());
                        assert_eq!(send_back_addr, "/memory".parse().unwrap());
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
        let t = DummyTransport::new();
        let addr1 = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
        let addr2 = "/ip4/127.0.0.1/tcp/4321".parse::<Multiaddr>().expect("bad multiaddr");
        let expected_addrs = vec![addr1.to_string(), addr2.to_string()];

        let mut ls = ListenersStream::new(t);
        ls.listen_on(addr1).expect("listen_on failed");
        ls.listen_on(addr2).expect("listen_on failed");

        let listener_addrs = ls.listeners().map(|ma| ma.to_string() ).collect::<Vec<String>>();
        assert_eq!(listener_addrs, expected_addrs);
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
        let addr = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
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
        t.set_initial_listener_state(ListenerState::Ok(Async::Ready(Some( (peer_id, muxer) ))));
        let mut ls = ListenersStream::new(t);

        let addr1 = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
        let addr2 = "/ip4/127.0.0.2/tcp/4321".parse::<Multiaddr>().expect("bad multiaddr");

        ls.listen_on(addr1).expect("listen_on works");
        ls.listen_on(addr2).expect("listen_on works");
        assert_eq!(ls.listeners.len(), 2);

        assert_matches!(ls.poll(), Async::Ready(listeners_event) => {
            assert_matches!(listeners_event, ListenersEvent::Incoming{mut upgrade, listen_addr, ..} => {
                assert_eq!(listen_addr.to_string(), "/ip4/127.0.0.2/tcp/4321");
                assert_matches!(upgrade.poll().unwrap(), Async::Ready(tup) => {
                    assert_eq!(tup, expected_output)
                });
            })
        });

        assert_matches!(ls.poll(), Async::Ready(listeners_event) => {
            assert_matches!(listeners_event, ListenersEvent::Incoming{mut upgrade, listen_addr, ..} => {
                assert_eq!(listen_addr.to_string(), "/ip4/127.0.0.1/tcp/1234");
                assert_matches!(upgrade.poll().unwrap(), Async::Ready(tup) => {
                    assert_eq!(tup, expected_output)
                });
            })
        });

        set_listener_state(&mut ls, 1, ListenerState::Ok(Async::NotReady));
        assert_matches!(ls.poll(), Async::Ready(listeners_event) => {
            assert_matches!(listeners_event, ListenersEvent::Incoming{mut upgrade, listen_addr, ..} => {
                assert_eq!(listen_addr.to_string(), "/ip4/127.0.0.1/tcp/1234");
                assert_matches!(upgrade.poll().unwrap(), Async::Ready(tup) => {
                    assert_eq!(tup, expected_output)
                });
            })
        });

    }

    #[test]
    fn listener_stream_poll_with_closed_listener_emits_closed_event() {
        let t = DummyTransport::new();
        let addr = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
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
        t.set_initial_listener_state(ListenerState::Ok(Async::Ready(Some( (peer_id, muxer) ))));
        let addr = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
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
        t.set_initial_listener_state(ListenerState::Ok(Async::Ready(Some( (peer_id.clone(), muxer) ))));        let mut ls = ListenersStream::new(t);

        // Create 4 Listeners
        for n in 0..4 {
            let addr = format!("/ip4/127.0.0.{}/tcp/{}", n, n).parse::<Multiaddr>().expect("bad multiaddr");
            ls.listen_on(addr).expect("listen_on failed");
        }

        // Poll() processes listeners in reverse order. Each listener is polled
        // in turn.
        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming{listen_addr, ..}) => {
                assert_eq!(listen_addr.to_string(), format!("/ip4/127.0.0.{}/tcp/{}", n, n))
            })
        }

        // Doing it again yields them in the same order
        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming{listen_addr, ..}) => {
                assert_eq!(listen_addr.to_string(), format!("/ip4/127.0.0.{}/tcp/{}", n, n))
            })
        }

        // Make last listener NotReady; it will become the first element and
        // retried after trying the other Listeners.
        set_listener_state(&mut ls, 3, ListenerState::Ok(Async::NotReady));
        for n in (0..3).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming{listen_addr, ..}) => {
                assert_eq!(listen_addr.to_string(), format!("/ip4/127.0.0.{}/tcp/{}", n, n))
            })
        }

        for n in (0..3).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming{listen_addr, ..}) => {
                assert_eq!(listen_addr.to_string(), format!("/ip4/127.0.0.{}/tcp/{}", n, n))
            })
        }

        // Turning the last listener back on means we now have 4 "good"
        // listeners, and each get their turn.
        set_listener_state(
            &mut ls, 3,
            ListenerState::Ok(Async::Ready(Some( (peer_id, DummyMuxer::new()) )))
        );
        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming{listen_addr, ..}) => {
                assert_eq!(listen_addr.to_string(), format!("/ip4/127.0.0.{}/tcp/{}", n, n))
            })
        }
    }

    #[test]
    fn listener_stream_poll_processes_listeners_in_turn() {
        let mut t = DummyTransport::new();
        let peer_id = PeerId::random();
        let muxer = DummyMuxer::new();
        t.set_initial_listener_state(ListenerState::Ok(Async::Ready(Some( (peer_id, muxer) ))));
        let mut ls = ListenersStream::new(t);
        for n in 0..4 {
            let addr = format!("/ip4/127.0.0.{}/tcp/{}", n, n).parse::<Multiaddr>().expect("bad multiaddr");
            ls.listen_on(addr).expect("listen_on failed");
        }

        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(ListenersEvent::Incoming{listen_addr, ..}) => {
                assert_eq!(listen_addr.to_string(), format!("/ip4/127.0.0.{}/tcp/{}", n, n));
            });
            set_listener_state(&mut ls, 0, ListenerState::Ok(Async::NotReady));
        }
        // All Listeners are NotReady, so poll yields NotReady
        assert_matches!(ls.poll(), Async::NotReady);
    }
}
