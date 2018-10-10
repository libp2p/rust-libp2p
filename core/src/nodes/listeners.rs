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

use futures::prelude::*;
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
        }
    }

    /// Same as `new`, but pre-allocates enough memory for the given number of
    /// simultaneous listeners.
    #[inline]
    pub fn with_capacity(transport: TTrans, capacity: usize) -> Self {
        ListenersStream {
            transport,
            listeners: Vec::with_capacity(capacity),
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
    pub fn poll(&mut self) -> Async<Option<ListenersEvent<TTrans>>> {
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
                    return Async::Ready(Some(ListenersEvent::Incoming {
                        upgrade,
                        listen_addr,
                    }));
                }
                Ok(Async::Ready(None)) => {
                    return Async::Ready(Some(ListenersEvent::Closed {
                        listen_addr: listener.address,
                        listener: listener.listener,
                        result: Ok(()),
                    }));
                }
                Err(err) => {
                    return Async::Ready(Some(ListenersEvent::Closed {
                        listen_addr: listener.address,
                        listener: listener.listener,
                        result: Err(err),
                    }));
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
        Ok(self.poll())
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
    use std::io;
    use futures::{future::{self}, stream};
    use tests::dummy_transport::{DummyTransport, ListenerState};

	fn set_listener_state(ls: &mut ListenersStream<DummyTransport>, idx: usize, state: ListenerState) {
		let l = &mut ls.listeners[idx];
		l.listener =
			match state {
				ListenerState::Error => {
					let stream = stream::poll_fn(|| future::err(io::Error::new(io::ErrorKind::Other, "oh noes")).poll() );
					Box::new(stream)
				}
				ListenerState::Ok(async) => {
					match async {
						Async::NotReady => {
							let stream = stream::poll_fn(|| Ok(Async::NotReady));
							Box::new(stream)
						}
						Async::Ready(Some(n)) => {
							let addr = l.address.clone();
							let stream = stream::iter_ok(n..)
								.map(move |stream| future::ok( (stream, future::ok(addr.clone())) ));
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

    #[test]
    fn listener_stream_returns_transport() {
        let t = DummyTransport::new();
        let ls = ListenersStream::new(t);
        assert_eq!(ls.transport(), &t);
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
        t.set_initial_listener_state(ListenerState::Ok(Async::Ready(Some(1))));
        let addr1 = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
        let addr2 = "/ip4/127.0.0.2/tcp/4321".parse::<Multiaddr>().expect("bad multiaddr");
        let mut ls = ListenersStream::new(t);
        ls.listen_on(addr1).expect("listen_on failed");
        ls.listen_on(addr2).expect("listen_on failed");

        assert_matches!(ls.poll(), Async::Ready(Some(listeners_event)) => {
            assert_matches!(listeners_event, ListenersEvent::Incoming{mut upgrade, listen_addr} => {
                assert_eq!(listen_addr.to_string(), "/ip4/127.0.0.2/tcp/4321");
                assert_matches!(upgrade.poll().unwrap(), Async::Ready(tup) => {
                    assert_matches!(tup, (1, _))
                });
            })
        });
        // TODO: When several listeners are continuously Async::Ready –
        // admittetdly a corner case – the last one is processed first and then
        // put back *last* on the pile. This means that at the next poll() it
        // will get polled again and if it always has data to yield, it will
        // effectively block all other listeners from being "heard". One way
        // around this is to switch to using a `VecDeque` to keep the listeners
        // collection, and instead of pushing the processed item to the end of
        // the list, stick it on top so that it'll be processed *last* instead
        // during the next poll. This might also get us a performance win as
        // even in the normal case, the most recently polled listener is more
        // unlikely to have anything to yield than the others so we might avoid
        // a few unneeded poll calls.

        // Make the second listener return NotReady so we get the first listener next poll()
        set_listener_state(&mut ls, 1, ListenerState::Ok(Async::NotReady));
        assert_matches!(ls.poll(), Async::Ready(Some(listeners_event)) => {
            assert_matches!(listeners_event, ListenersEvent::Incoming{mut upgrade, listen_addr} => {
                assert_eq!(listen_addr.to_string(), "/ip4/127.0.0.1/tcp/1234");
                assert_matches!(upgrade.poll().unwrap(), Async::Ready(tup) => {
                    assert_matches!(tup, (1, _))
                });
            })
        });
        assert_eq!(ls.listeners.len(), 2);
    }

    #[test]
    fn listener_stream_poll_with_closed_listener_emits_closed_event() {
        let t = DummyTransport::new();
        let addr = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
        let mut ls = ListenersStream::new(t);
        ls.listen_on(addr).expect("listen_on failed");
        set_listener_state(&mut ls, 0, ListenerState::Ok(Async::Ready(None)));
        assert_matches!(ls.poll(), Async::Ready(Some(listeners_event)) => {
            assert_matches!(listeners_event, ListenersEvent::Closed{..})
        });
        assert_eq!(ls.listeners.len(), 0); // it's gone
    }

    #[test]
    fn listener_stream_poll_with_erroring_listener_emits_closed_event() {
        let mut t = DummyTransport::new();
        t.set_initial_listener_state(ListenerState::Ok(Async::Ready(Some(1))));
        let addr = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().expect("bad multiaddr");
        let mut ls = ListenersStream::new(t);
        ls.listen_on(addr).expect("listen_on failed");
        set_listener_state(&mut ls, 0, ListenerState::Error); // simulate an error on the socket
        assert_matches!(ls.poll(), Async::Ready(Some(listeners_event)) => {
            assert_matches!(listeners_event, ListenersEvent::Closed{..})
        });
        assert_eq!(ls.listeners.len(), 0); // it's gone
    }

    #[test]
    fn listener_stream_poll_chatty_listeners_may_drown_others() {
        let mut t = DummyTransport::new();
        t.set_initial_listener_state(ListenerState::Ok(Async::Ready(Some(1))));
        let mut ls = ListenersStream::new(t);
        for n in 0..4 {
            let addr = format!("/ip4/127.0.0.{}/tcp/123{}", n, n).parse::<Multiaddr>().expect("bad multiaddr");
            ls.listen_on(addr).expect("listen_on failed");
        }

        // polling processes listeners in reverse order
        // Only the last listener ever gets processed
        for _n in 0..10 {
            assert_matches!(ls.poll(), Async::Ready(Some(ListenersEvent::Incoming{listen_addr, ..})) => {
                assert_eq!(listen_addr.to_string(), "/ip4/127.0.0.3/tcp/1233")
            })
        }
        // Make last listener NotReady so now only the third listener is processed
        set_listener_state(&mut ls, 3, ListenerState::Ok(Async::NotReady));
        for _n in 0..10 {
            assert_matches!(ls.poll(), Async::Ready(Some(ListenersEvent::Incoming{listen_addr, ..})) => {
                assert_eq!(listen_addr.to_string(), "/ip4/127.0.0.2/tcp/1232")
            })
        }
    }

    #[test]
    fn listener_stream_poll_processes_listeners_as_expected_if_they_are_not_yielding_continuously() {
        let mut t = DummyTransport::new();
        t.set_initial_listener_state(ListenerState::Ok(Async::Ready(Some(1))));
        let mut ls = ListenersStream::new(t);
        for n in 0..4 {
            let addr = format!("/ip4/127.0.0.{}/tcp/123{}", n, n).parse::<Multiaddr>().expect("bad multiaddr");
            ls.listen_on(addr).expect("listen_on failed");
        }
        // If the listeners do not yield items continuously (the normal case) we
        // process them in the expected, reverse, order.
        for n in (0..4).rev() {
            assert_matches!(ls.poll(), Async::Ready(Some(ListenersEvent::Incoming{listen_addr, ..})) => {
                assert_eq!(listen_addr.to_string(), format!("/ip4/127.0.0.{}/tcp/123{}", n, n));
            });
            // kick the last listener (current) to NotReady state
            set_listener_state(&mut ls, 3, ListenerState::Ok(Async::NotReady));
        }
    }
}
