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

use futures::sync::{oneshot, mpsc};
use futures::{Async, Future, IntoFuture, Poll, Stream, future, stream};
use parking_lot::Mutex;
use std::fmt;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::sync::Arc;
use tokio_executor;
use {Multiaddr, MuxedTransport, Transport};

/// Creates a swarm.
///
/// Requires an upgraded transport, and a function or closure that will turn the upgrade into a
/// `Future` that produces a `()`.
///
/// Produces a `SwarmController` and an implementation of `Future`. The controller can be used to
/// control, and the `Future` must be driven to completion in order for things to work.
pub fn swarm<T, H, F>(
    transport: T,
    mut handler: H,
) -> (SwarmController<T, F::Future>, SwarmEvents<F::Future>)
where
    T: MuxedTransport + Clone + 'static, // TODO: 'static :-/
    T::Incoming: Send,
    T::IncomingUpgrade: Send,
    T::MultiaddrFuture: Send,
    H: FnMut(T::Output, Box<Future<Item = Multiaddr, Error = IoError> + Send>) -> F + Send + 'static,
    F: IntoFuture<Item = (), Error = IoError>,
    F::Future: Send + 'static,
{
    let handler = Arc::new(Mutex::new(move |out, addr| handler(out, addr).into_future()));
    let (ev_tx, ev_rx) = mpsc::unbounded();
    let (ts_tx, ts_rx) = mpsc::unbounded();

    // Create the task that handles incoming substreams.
    {
        let transport = transport.clone();
        let events_tx = ev_tx.clone();
        let handler = handler.clone();
        let mut next_incoming = transport.next_incoming();
        let future = future::poll_fn(move || {
            match next_incoming.poll() {
                Ok(Async::Ready(incoming)) => {
                    let events_tx = events_tx.clone();
                    let handler = handler.clone();
                    trace!("Incoming muxed connection");

                    tokio_executor::spawn(incoming.then(move |outcome| {
                        match outcome {
                            Ok((output, client_addr)) => {
                                trace!("Successfully negotiated incoming connection");
                                let mut handler = handler.lock();
                                let handler = &mut *handler;
                                let client_addr = Box::new(client_addr) as
                                    Box<Future<Item = _, Error = _> + Send>;
                                let to_process = (*handler)(output, client_addr).into_future();
                                spawn_to_process(to_process, events_tx);
                                Ok(())
                            },
                            Err(err) => {
                                let _ = events_tx.unbounded_send(SwarmEvent::ListenerUpgradeError(err));
                                Ok(())
                            },
                        }
                    }));
                    Ok(Async::Ready(()))
                },
                Ok(Async::NotReady) => {
                    Ok(Async::NotReady)
                },
                Err(err) => {
                    let _ = events_tx.unbounded_send(SwarmEvent::IncomingError(err));
                    Ok(Async::Ready(()))
                },
            }
        });
        let _ = ts_tx.unbounded_send(Box::new(future) as Box<_>);
    }

    let controller = SwarmController { transport, handler, sender: ev_tx, to_spawn: ts_tx };
    let events = SwarmEvents { inner: ev_rx, to_spawn: ts_rx.fuse() };
    (controller, events)
}

/// Allows control of what the swarm is doing.
pub struct SwarmController<T, F> where T: Transport {
    /// Transport used to dial or listen.
    transport: T,

    /// Handler for incoming connections.
    handler: Arc<Mutex<FnMut(T::Output, Box<Future<Item = Multiaddr, Error = IoError> + Send>) -> F + Send>>,

    /// Sender to use to propagate events to the `SwarmEvents`.
    sender: mpsc::UnboundedSender<SwarmEvent<F>>,

    /// Sender for futures to dispatch to execute.
    to_spawn: mpsc::UnboundedSender<Box<Future<Item = (), Error = ()> + Send>>,
}

impl<T, F> fmt::Debug for SwarmController<T, F>
where T: Transport + fmt::Debug
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_tuple("SwarmController")
            .field(&self.transport)
            .finish()
    }
}

impl<T, F> Clone for SwarmController<T, F>
where
    T: Transport + Clone
{
    fn clone(&self) -> Self {
        SwarmController {
            transport: self.transport.clone(),
            handler: self.handler.clone(),
            sender: self.sender.clone(),
            to_spawn: self.to_spawn.clone(),
        }
    }
}

impl<T, F> SwarmController<T, F>
where
    T: MuxedTransport + Clone + 'static, // TODO: 'static :-/
    T::Dial: Send,
    T::MultiaddrFuture: Send,
    T::Listener: Send,
    T::ListenerUpgrade: Send,
    T::Output: Send,
    F: Future<Item = (), Error = IoError> + Send + 'static,
{
    /// Asks the swarm to dial the node with the given multiaddress. The connection is then
    /// upgraded using the `upgrade`, and the output is sent to the handler that was passed when
    /// calling `swarm`.
    ///
    /// Returns a future that is signalled once the closure in the `swarm` has returned its future.
    /// Therefore if the closure in the swarm has some side effect (eg. write something in a
    /// variable), this side effect will be observable when this future succeeds.
    #[inline]
    pub fn dial<Du>(
        &self,
        multiaddr: Multiaddr,
        transport: Du,
    ) -> Result<impl Future<Item = (), Error = IoError>, Multiaddr>
    where
        Du: Transport + Clone + 'static, // TODO: 'static :-/
        Du::Dial: Send,
        Du::MultiaddrFuture: Send,
        Du::Listener: Send,
        Du::ListenerUpgrade: Send,
        Du::Output: Send + Into<T::Output>,
    {
        self.dial_then(multiaddr, transport, |v| v)
    }

    /// Internal version of `dial` that allows adding a closure that is called after either the
    /// dialing fails or the handler has been called with the resulting future.
    ///
    /// The returned future is filled with the output of `then`.
    pub(crate) fn dial_then<Du, TThen>(
        &self,
        multiaddr: Multiaddr,
        transport: Du,
        then: TThen,
    ) -> Result<impl Future<Item = (), Error = IoError>, Multiaddr>
    where
        Du: Transport + Clone + 'static, // TODO: 'static :-/
        Du::Dial: Send,
        Du::MultiaddrFuture: Send,
        Du::Listener: Send,
        Du::ListenerUpgrade: Send,
        Du::Output: Send + Into<T::Output>,
        TThen: FnOnce(Result<(), IoError>) -> Result<(), IoError> + Send + 'static,
    {
        trace!("Swarm dialing {}", multiaddr);

        let dial = match transport.clone().dial(multiaddr.clone()) {
            Ok(d) => d,
            Err((_, addr)) => return Err(addr)
        };

        let (rx, mut then) = {
            let (tx, rx) = oneshot::channel();
            let mut then = Some(move |val| {
                let _ = tx.send(then(val));
            });
            // Unfortunately the `Box<FnOnce(_)>` type is still unusable in Rust right now,
            // so we use a `Box<FnMut(_)>` instead and panic if it is called multiple times.
            let then = Box::new(move |val: Result<(), IoError>| {
                let then = then
                    .take()
                    .expect("The Boxed FnMut should only be called once");
                then(val);
            }) as Box<FnMut(_) + Send>;
            (rx, then)
        };

        let events_tx = self.sender.clone();
        let handler = self.handler.clone();

        let dial = dial.then(move |result| match result {
            Ok((output, client_addr)) => {
                trace!("Successfully dialed address {}", multiaddr);
                let client_addr = Box::new(client_addr) as Box<Future<Item = _, Error = _> + Send>;
                let mut handler = handler.lock();
                let handler = &mut *handler;
                let to_process = (*handler)(output.into(), client_addr);
                then(Ok(()));
                spawn_to_process(to_process, events_tx);
                Ok(())
            }
            Err(err) => {
                debug!("Error in dialer upgrade: {:?}", err);
                let err_clone = IoError::new(err.kind(), err.to_string());
                then(Err(err));
                let _ = events_tx.unbounded_send(SwarmEvent::DialFailed {
                    client_addr: multiaddr,
                    error: err_clone,
                });
                Ok(())
            }
        });

        let _ = self.to_spawn.unbounded_send(Box::new(dial) as Box<_>);

        Ok(rx.then(|result| match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(IoError::new(
                IoErrorKind::ConnectionAborted,
                "dial cancelled the swarm future has been destroyed",
            )),
        }))
    }

    /// Adds a multiaddr to listen on. All the incoming connections will use the `upgrade` that
    /// was passed to `swarm`.
    // TODO: add a way to cancel a listener
    pub fn listen_on(&self, multiaddr: Multiaddr) -> Result<Multiaddr, Multiaddr> {
        let events_tx = self.sender.clone();
        let events_tx2 = self.sender.clone();
        let handler = self.handler.clone();

        match self.transport.clone().listen_on(multiaddr) {
            Ok((listener, new_addr)) => {
                trace!("Swarm listening on {}", new_addr);
                let listen_addr = new_addr.clone();

                let listener = listener
                    .for_each(move |incoming| {
                        let events_tx = events_tx.clone();
                        let handler = handler.clone();
                        trace!("Incoming connection on listener");

                        tokio_executor::spawn(incoming.then(move |outcome| {
                            match outcome {
                                Ok((output, client_addr)) => {
                                    trace!("Successfully negotiated incoming connection");
                                    let mut handler = handler.lock();
                                    let handler = &mut *handler;
                                    let client_addr = Box::new(client_addr) as
                                        Box<Future<Item = _, Error = _> + Send>;
                                    let to_process = (*handler)(output, client_addr).into_future();
                                    spawn_to_process(to_process, events_tx);
                                    Ok(())
                                },
                                Err(err) => {
                                    let _ = events_tx.unbounded_send(SwarmEvent::ListenerUpgradeError(err));
                                    Ok(())
                                },
                            }
                        }));
                        Ok(())
                    })
                    .then(move |outcome| {
                        let _ = match outcome {
                            Ok(()) => {
                                trace!("Listener to {} gracefully closed", listen_addr);
                                let _ = events_tx2.unbounded_send(SwarmEvent::ListenerClosed { listen_addr });
                            },
                            Err(error) => {
                                trace!("Listener to {} errored: {:?}", listen_addr, error);
                                let _ = events_tx2.unbounded_send(SwarmEvent::ListenerError { listen_addr, error });
                            },
                        };
                        Ok(())
                    });

                let _ = self.to_spawn.unbounded_send(Box::new(listener) as Box<_>);
                Ok(new_addr)
            }
            Err((_, multiaddr)) => Err(multiaddr),
        }
    }
}

/// Spawns a task that processes `to_process` and reports back to `events`.
fn spawn_to_process<F>(to_process: F, events: mpsc::UnboundedSender<SwarmEvent<F>>)
where F: Future<Item = (), Error = IoError> + Send + 'static
{
    let mut to_process = Some(to_process);
    let fut = future::poll_fn(move || {
        let mut local_toprocess = to_process.take().expect("Future polled after it's been completed");
        match local_toprocess.poll() {
            Ok(Async::Ready(())) => {
                trace!("Substream gracefully closed");
                let _ = events.unbounded_send(SwarmEvent::HandlerFinished {
                    handler_future: local_toprocess,
                });
                Ok(Async::Ready(()))
            },
            Ok(Async::NotReady) => {
                to_process = Some(local_toprocess);
                Ok(Async::NotReady)
            },
            Err(error) => {
                trace!("Substream errored: {:?}", error);
                let _ = events.unbounded_send(SwarmEvent::HandlerError {
                    handler_future: local_toprocess,
                    error,
                });
                Ok(Async::Ready(()))
            },
        }
    });

    tokio_executor::spawn(Box::new(fut) as Box<_>);
}

/// Stream of the events that happen the swarm.
#[must_use = "futures do nothing unless polled"]
pub struct SwarmEvents<F> {
    inner: mpsc::UnboundedReceiver<SwarmEvent<F>>,
    to_spawn: stream::Fuse<mpsc::UnboundedReceiver<Box<Future<Item = (), Error = ()> + Send>>>,
}

impl<F> Stream for SwarmEvents<F> {
    type Item = SwarmEvent<F>;
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            match self.to_spawn.poll() {
                Ok(Async::Ready(Some(val))) => {
                    trace!("Spawning processing future");
                    tokio_executor::spawn(val);
                },
                Ok(Async::Ready(None)) => break,
                Ok(Async::NotReady) => break,
                Err(()) => unreachable!("An UnboundedReceiver can never error")
            }
        }

        match self.inner.poll() {
            Ok(val) => Ok(val),
            Err(()) => unreachable!("An UnboundedReceiver can never error")
        }
    }
}

/// Event that happens in the swarm.
#[derive(Debug)]
pub enum SwarmEvent<F> {
    /// An error has happened while polling the muxed transport for incoming connections.
    IncomingError(IoError),

    /// A listener has gracefully closed.
    ListenerClosed {
        /// Address the listener was listening on.
        listen_addr: Multiaddr,
    },

    /// A listener has stopped because it produced an error.
    ListenerError {
        /// Address the listener was listening on.
        listen_addr: Multiaddr,
        /// The error that happened.
        error: IoError,
    },

    /// An error happened while upgrading an incoming connection.
    ListenerUpgradeError(IoError),

    /// Failed to dial a remote address.
    DialFailed {
        /// Address we were trying to dial.
        client_addr: Multiaddr,
        /// Error that happened.
        error: IoError,
    },

    /// A future returned by the handler has finished.
    HandlerFinished {
        /// The future originally returned by the handler.
        handler_future: F,
    },

    /// A future returned by the handler has produced an error.
    HandlerError {
        /// The future originally returned by the handler.
        handler_future: F,
        /// The error that happened.
        error: IoError,
    },
}

#[cfg(test)]
mod tests {
    use futures::{future, Future, Stream};
    use rand;
    use std::io::Error as IoError;
    use std::sync::{atomic, Arc};
    use swarm;
    use tokio::runtime::current_thread;
    use transport::{self, DeniedTransport, Transport};

    #[test]
    fn transport_error_propagation_listen() {
        let (swarm_ctrl, _swarm_future) = swarm(DeniedTransport, |_, _| future::empty());
        assert!(
            swarm_ctrl
                .listen_on("/ip4/127.0.0.1/tcp/10000".parse().unwrap())
                .is_err()
        );
    }

    #[test]
    fn transport_error_propagation_dial() {
        let (swarm_ctrl, _swarm_future) = swarm(DeniedTransport, |_, _| future::empty());
        let addr = "/ip4/127.0.0.1/tcp/10000".parse().unwrap();
        assert!(swarm_ctrl.dial(addr, DeniedTransport).is_err());
    }

    #[test]
    fn basic_dial() {
        let (tx, rx) = transport::connector();

        let reached_tx = Arc::new(atomic::AtomicBool::new(false));
        let reached_tx2 = reached_tx.clone();

        let reached_rx = Arc::new(atomic::AtomicBool::new(false));
        let reached_rx2 = reached_rx.clone();

        let (swarm_ctrl1, swarm_future1) = swarm(rx.with_dummy_muxing(), |_, _| {
            reached_rx2.store(true, atomic::Ordering::SeqCst);
            future::empty()
        });
        swarm_ctrl1.listen_on("/memory".parse().unwrap()).unwrap();

        let (swarm_ctrl2, swarm_future2) = swarm(tx.clone().with_dummy_muxing(), |_, _| {
            reached_tx2.store(true, atomic::Ordering::SeqCst);
            future::empty()
        });

        let dial_success = swarm_ctrl2.dial("/memory".parse().unwrap(), tx).unwrap();
        let future = swarm_future2
            .for_each(|_| Ok(()))
            .select(swarm_future1.for_each(|_| Ok(())))
            .map(|_| ())
            .map_err(|(err, _)| err)
            .select(dial_success)
            .map(|_| ())
            .map_err(|(err, _)| err);

        current_thread::Runtime::new()
            .unwrap()
            .block_on(future)
            .unwrap();
        assert!(reached_tx.load(atomic::Ordering::SeqCst));
        assert!(reached_rx.load(atomic::Ordering::SeqCst));
    }

    #[test]
    fn dial_multiple_times() {
        let (tx, rx) = transport::connector();
        let reached = Arc::new(atomic::AtomicUsize::new(0));
        let reached2 = reached.clone();
        let (swarm_ctrl, swarm_future) = swarm(rx.with_dummy_muxing(), |_, _| {
            reached2.fetch_add(1, atomic::Ordering::SeqCst);
            future::empty()
        });
        swarm_ctrl.listen_on("/memory".parse().unwrap()).unwrap();
        let num_dials = 20000 + rand::random::<usize>() % 20000;
        let mut dials = Vec::new();
        for _ in 0..num_dials {
            let f = swarm_ctrl
                .dial("/memory".parse().unwrap(), tx.clone())
                .unwrap();
            dials.push(f);
        }
        let future = future::join_all(dials)
            .map(|_| ())
            .select(swarm_future.for_each(|_| Ok(())))
            .map_err(|(err, _)| err);
        current_thread::Runtime::new()
            .unwrap()
            .block_on(future)
            .unwrap();
        assert_eq!(reached.load(atomic::Ordering::SeqCst), num_dials);
    }

    #[test]
    fn future_isnt_dropped() {
        // Tests that the future in the closure isn't being dropped.
        let (tx, rx) = transport::connector();
        let (swarm_ctrl, swarm_future) = swarm(rx.with_dummy_muxing(), |_, _| {
            future::empty().then(|_: Result<(), ()>| -> Result<(), IoError> { panic!() }) // <-- the test
        });
        swarm_ctrl.listen_on("/memory".parse().unwrap()).unwrap();
        let dial_success = swarm_ctrl.dial("/memory".parse().unwrap(), tx).unwrap();
        let future = dial_success
            .select(swarm_future.for_each(|_| Ok(())))
            .map_err(|(err, _)| err);
        current_thread::Runtime::new()
            .unwrap()
            .block_on(future)
            .unwrap();
    }
}
