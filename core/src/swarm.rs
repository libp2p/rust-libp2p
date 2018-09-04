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

use futures::stream::StreamFuture;
use futures::sync::oneshot;
use futures::{Async, Future, IntoFuture, Poll, Stream};
use futures::task;
use parking_lot::Mutex;
use std::fmt;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::sync::Arc;
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
    handler: H,
) -> (SwarmController<T, F::Future>, SwarmEvents<T, F::Future, H>)
where
    T: MuxedTransport + Clone + 'static, // TODO: 'static :-/
    H: FnMut(T::Output, Box<Future<Item = Multiaddr, Error = IoError>>) -> F,
    F: IntoFuture<Item = (), Error = IoError>,
{
    let shared = Arc::new(Mutex::new(Shared {
        next_incoming: transport.clone().next_incoming(),
        listeners: Vec::new(),
        listeners_upgrade: Vec::new(),
        dialers: Vec::new(),
        to_process: Vec::new(),
        task_to_notify: None,
    }));

    let future = SwarmEvents {
        transport: transport.clone(),
        shared: shared.clone(),
        handler: handler,
    };

    let controller = SwarmController {
        transport,
        shared,
    };

    (controller, future)
}

/// Allows control of what the swarm is doing.
pub struct SwarmController<T, F>
where
    T: MuxedTransport + 'static, // TODO: 'static :-/
{
    /// Shared between the swarm infrastructure.
    shared: Arc<Mutex<Shared<T, F>>>,

    /// Transport used to dial or listen.
    transport: T,
}

impl<T, F> fmt::Debug for SwarmController<T, F>
where
    T: fmt::Debug + MuxedTransport + 'static, // TODO: 'static :-/
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_tuple("SwarmController")
            .field(&self.transport)
            .finish()
    }
}

impl<T, F> Clone for SwarmController<T, F>
where
    T: MuxedTransport + Clone + 'static, // TODO: 'static :-/
{
    fn clone(&self) -> Self {
        SwarmController {
            transport: self.transport.clone(),
            shared: self.shared.clone(),
        }
    }
}

impl<T, F> SwarmController<T, F>
where
    T: MuxedTransport + Clone + 'static, // TODO: 'static :-/
    F: 'static,
{
    /// Asks the swarm to dial the node with the given multiaddress. The connection is then
    /// upgraded using the `upgrade`, and the output is sent to the handler that was passed when
    /// calling `swarm`.
    ///
    /// Returns a future that is signalled once the closure in the `swarm` has returned its future.
    /// Therefore if the closure in the swarm has some side effect (eg. write something in a
    /// variable), this side effect will be observable when this future succeeds.
    #[inline]
    pub fn dial<Du>(&self, multiaddr: Multiaddr, transport: Du)
        -> Result<impl Future<Item = (), Error = IoError>, Multiaddr>
    where
        Du: Transport + 'static, // TODO: 'static :-/
        Du::Output: Into<T::Output>,
    {
        self.dial_then(multiaddr, transport, |v| v)
    }

    /// Internal version of `dial` that allows adding a closure that is called after either the
    /// dialing fails or the handler has been called with the resulting future.
    ///
    /// The returned future is filled with the output of `then`.
    pub(crate) fn dial_then<Du, TThen>(&self, multiaddr: Multiaddr, transport: Du, then: TThen)
        -> Result<impl Future<Item = (), Error = IoError>, Multiaddr>
    where
        Du: Transport + 'static, // TODO: 'static :-/
        Du::Output: Into<T::Output>,
        TThen: FnOnce(Result<(), IoError>) -> Result<(), IoError> + 'static,
    {
        trace!("Swarm dialing {}", multiaddr);

        match transport.dial(multiaddr.clone()) {
            Ok(dial) => {
                let (tx, rx) = oneshot::channel();
                let mut then = Some(move |val| {
                    let _ = tx.send(then(val));
                });
                // Unfortunately the `Box<FnOnce(_)>` type is still unusable in Rust right now,
                // so we use a `Box<FnMut(_)>` instead and panic if it is called multiple times.
                let mut then = Box::new(move |val: Result<(), IoError>| {
                    let then = then.take().expect("The Boxed FnMut should only be called once");
                    then(val);
                }) as Box<FnMut(_)>;

                let dial = dial.then(|result| {
                    match result {
                        Ok((output, client_addr)) => {
                            let client_addr = Box::new(client_addr) as Box<Future<Item = _, Error = _>>;
                            Ok((output.into(), then, client_addr))
                        }
                        Err(err) => {
                            debug!("Error in dialer upgrade: {:?}", err);
                            let err_clone = IoError::new(err.kind(), err.to_string());
                            then(Err(err));
                            Err(err_clone)
                        }
                    }
                });

                let mut shared = self.shared.lock();
                shared.dialers.push((multiaddr, Box::new(dial) as Box<_>));
                if let Some(task) = shared.task_to_notify.take() {
                    task.notify();
                }

                Ok(rx.then(|result| {
                    match result {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(err)) => Err(err),
                        Err(_) => Err(IoError::new(IoErrorKind::ConnectionAborted,
                            "dial cancelled the swarm future has been destroyed")),
                    }
                }))
            }
            Err((_, multiaddr)) => Err(multiaddr),
        }
    }

    /// Interrupts all dialing attempts to a specific multiaddress.
    ///
    /// Has no effect if the dialing attempt has already succeeded, in which case it will be
    /// dispatched to the handler.
    pub fn interrupt_dial(&self, multiaddr: &Multiaddr) {
        let mut shared = self.shared.lock();
        shared.dialers.retain(|dialer| {
            &dialer.0 != multiaddr
        });
    }

    /// Adds a multiaddr to listen on. All the incoming connections will use the `upgrade` that
    /// was passed to `swarm`.
    // TODO: add a way to cancel a listener
    pub fn listen_on(&self, multiaddr: Multiaddr) -> Result<Multiaddr, Multiaddr> {
        match self.transport.clone().listen_on(multiaddr) {
            Ok((listener, new_addr)) => {
                trace!("Swarm listening on {}", new_addr);
                let mut shared = self.shared.lock();
                let listener = Box::new(
                    listener.map(|f| {
                        let f = f.map(|(out, maf)| {
                            (out, Box::new(maf) as Box<Future<Item = Multiaddr, Error = IoError>>)
                        });

                        Box::new(f) as Box<Future<Item = _, Error = _>>
                    }),
                ) as Box<Stream<Item = _, Error = _>>;
                shared.listeners.push((new_addr.clone(), listener.into_future()));
                if let Some(task) = shared.task_to_notify.take() {
                    task.notify();
                }
                Ok(new_addr)
            }
            Err((_, multiaddr)) => Err(multiaddr),
        }
    }
}

/// Future that must be driven to completion in order for the swarm to work.
#[must_use = "futures do nothing unless polled"]
pub struct SwarmEvents<T, F, H>
where
    T: MuxedTransport + 'static, // TODO: 'static :-/
{
    /// Shared between the swarm infrastructure.
    shared: Arc<Mutex<Shared<T, F>>>,

    /// The transport used to dial.
    transport: T,

    /// Swarm handler.
    handler: H,
}

impl<T, H, If, F> Stream for SwarmEvents<T, F, H>
where
    T: MuxedTransport + Clone + 'static, // TODO: 'static :-/,
    H: FnMut(T::Output, Box<Future<Item = Multiaddr, Error = IoError>>) -> If,
    If: IntoFuture<Future = F, Item = (), Error = IoError>,
    F: Future<Item = (), Error = IoError> + 'static,        // TODO: 'static :-/
{
    type Item = SwarmEvent<F>;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut shared = self.shared.lock();
        let handler = &mut self.handler;

        loop {
            match shared.next_incoming.poll() {
                Ok(Async::Ready(connec)) => {
                    debug!("Swarm received new multiplexed incoming connection");
                    shared.next_incoming = self.transport.clone().next_incoming();
                    let connec = connec.map(|(out, maf)| {
                        (out, Box::new(maf) as Box<Future<Item = Multiaddr, Error = IoError>>)
                    });
                    shared.listeners_upgrade.push(Box::new(connec) as Box<_>);
                }
                Ok(Async::NotReady) => break,
                Err(err) => {
                    // TODO: should that stop everything?
                    debug!("Error in multiplexed incoming connection: {:?}", err);
                    shared.next_incoming = self.transport.clone().next_incoming();
                    return Ok(Async::Ready(Some(SwarmEvent::IncomingError(err))));
                }
            }
        }

        // We remove each element from `shared.listeners` one by one and add them back only
        // if relevant.
        for n in (0 .. shared.listeners.len()).rev() {
            let (listen_addr, mut listener) = shared.listeners.swap_remove(n);
            loop {
                match listener.poll() {
                    Ok(Async::Ready((Some(upgrade), remaining))) => {
                        trace!("Swarm received new connection on listener socket");
                        shared.listeners_upgrade.push(upgrade);
                        listener = remaining.into_future();
                    }
                    Ok(Async::Ready((None, _))) => {
                        debug!("Listener closed gracefully");
                        return Ok(Async::Ready(Some(SwarmEvent::ListenerClosed {
                            listen_addr
                        })));
                    },
                    Err((error, _)) => {
                        debug!("Error in listener: {:?}", error);
                        return Ok(Async::Ready(Some(SwarmEvent::ListenerError {
                            listen_addr,
                            error,
                        })));
                    }
                    Ok(Async::NotReady) => {
                        shared.listeners.push((listen_addr, listener));
                        break;
                    }
                }
            }
        }

        // We remove each element from `shared.listeners_upgrade` one by one and add them back
        // only if relevant.
        for n in (0 .. shared.listeners_upgrade.len()).rev() {
            let mut listener_upgrade = shared.listeners_upgrade.swap_remove(n);
            match listener_upgrade.poll() {
                Ok(Async::Ready((output, client_addr))) => {
                    debug!("Successfully upgraded incoming connection");
                    // TODO: unlock mutex before calling handler, in order to avoid deadlocks if
                    // the user does something stupid
                    shared.to_process.push(handler(output, client_addr).into_future());
                }
                Err(err) => {
                    debug!("Error in listener upgrade: {:?}", err);
                    return Ok(Async::Ready(Some(SwarmEvent::ListenerUpgradeError(err))));
                }
                Ok(Async::NotReady) => {
                    shared.listeners_upgrade.push(listener_upgrade);
                },
            }
        }

        // We remove each element from `shared.dialers` one by one and add them back only
        // if relevant.
        for n in (0 .. shared.dialers.len()).rev() {
            let (client_addr, mut dialer) = shared.dialers.swap_remove(n);
            match dialer.poll() {
                Ok(Async::Ready((output, mut notifier, addr))) => {
                    trace!("Successfully upgraded dialed connection");
                    // TODO: unlock mutex before calling handler, in order to avoid deadlocks if
                    // the user does something stupid
                    shared.to_process.push(handler(output, addr).into_future());
                    notifier(Ok(()));
                }
                Err(error) => {
                    return Ok(Async::Ready(Some(SwarmEvent::DialFailed {
                        client_addr,
                        error,
                    })));
                },
                Ok(Async::NotReady) => {
                    shared.dialers.push((client_addr, dialer));
                },
            }
        }

        // We remove each element from `shared.to_process` one by one and add them back only
        // if relevant.
        for n in (0 .. shared.to_process.len()).rev() {
            let mut to_process = shared.to_process.swap_remove(n);
            match to_process.poll() {
                Ok(Async::Ready(())) => {
                    trace!("Future returned by swarm handler driven to completion");
                    return Ok(Async::Ready(Some(SwarmEvent::HandlerFinished {
                        handler_future: to_process,
                    })));
                }
                Err(error) => {
                    debug!("Error in processing: {:?}", error);
                    return Ok(Async::Ready(Some(SwarmEvent::HandlerError {
                        handler_future: to_process,
                        error,
                    })));
                }
                Ok(Async::NotReady) => {
                    shared.to_process.push(to_process);
                }
            }
        }

        // TODO: we never return `Ok(Ready)` because there's no way to know whether
        //       `next_incoming()` can produce anything more in the future ; also we would need to
        //       know when the controller has been dropped
        shared.task_to_notify = Some(task::current());
        Ok(Async::NotReady)
    }
}

// TODO: stronger typing
struct Shared<T, F> where T: MuxedTransport + 'static {
    /// Next incoming substream on the transport.
    next_incoming: T::Incoming,

    /// All the active listeners.
    listeners: Vec<(
        Multiaddr,
        StreamFuture<
            Box<
                Stream<
                    Item = Box<Future<Item = (T::Output, Box<Future<Item = Multiaddr, Error = IoError>>), Error = IoError>>,
                    Error = IoError,
                >,
            >,
        >,
    )>,

    /// Futures that upgrade an incoming listening connection to a full connection.
    listeners_upgrade:
        Vec<Box<Future<Item = (T::Output, Box<Future<Item = Multiaddr, Error = IoError>>), Error = IoError>>>,

    /// Futures that dial a remote address.
    ///
    /// Contains the address we dial, so that we can cancel it if necessary.
    dialers: Vec<(Multiaddr, Box<Future<Item = (T::Output, Box<FnMut(Result<(), IoError>)>, Box<Future<Item = Multiaddr, Error = IoError>>), Error = IoError>>)>,

    /// List of futures produced by the swarm closure. Must be processed to the end.
    to_process: Vec<F>,

    /// The task to notify whenever we add a new element in one of the lists.
    /// Necessary so that the task wakes up and the element gets polled.
    task_to_notify: Option<task::Task>,
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
    use futures::{Future, Stream, future};
    use rand;
    use transport::{self, DeniedTransport, Transport};
    use std::io::Error as IoError;
    use std::sync::{atomic, Arc};
    use swarm;
    use tokio::runtime::current_thread;

    #[test]
    fn transport_error_propagation_listen() {
        let (swarm_ctrl, _swarm_future) = swarm(DeniedTransport, |_, _| future::empty());
        assert!(swarm_ctrl.listen_on("/ip4/127.0.0.1/tcp/10000".parse().unwrap()).is_err());
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
        let future = swarm_future2.for_each(|_| Ok(()))
            .select(swarm_future1.for_each(|_| Ok(()))).map(|_| ()).map_err(|(err, _)| err)
            .select(dial_success).map(|_| ()).map_err(|(err, _)| err);

        current_thread::Runtime::new().unwrap().block_on(future).unwrap();
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
        for _ in 0 .. num_dials {
            let f = swarm_ctrl.dial("/memory".parse().unwrap(), tx.clone()).unwrap();
            dials.push(f);
        }
        let future = future::join_all(dials)
            .map(|_| ())
            .select(swarm_future.for_each(|_| Ok(())))
            .map_err(|(err, _)| err);
        current_thread::Runtime::new().unwrap().block_on(future).unwrap();
        assert_eq!(reached.load(atomic::Ordering::SeqCst), num_dials);
    }

    #[test]
    fn future_isnt_dropped() {
        // Tests that the future in the closure isn't being dropped.
        let (tx, rx) = transport::connector();
        let (swarm_ctrl, swarm_future) = swarm(rx.with_dummy_muxing(), |_, _| {
            future::empty()
                .then(|_: Result<(), ()>| -> Result<(), IoError> { panic!() })     // <-- the test
        });
        swarm_ctrl.listen_on("/memory".parse().unwrap()).unwrap();
        let dial_success = swarm_ctrl.dial("/memory".parse().unwrap(), tx).unwrap();
        let future = dial_success.select(swarm_future.for_each(|_| Ok(())))
            .map_err(|(err, _)| err);
        current_thread::Runtime::new().unwrap().block_on(future).unwrap();
    }
}
