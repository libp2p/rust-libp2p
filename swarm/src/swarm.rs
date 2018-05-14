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

use futures::stream::{FuturesUnordered, StreamFuture};
use futures::sync::mpsc;
use futures::{future, Async, Future, IntoFuture, Poll, Stream};
use std::fmt;
use std::io::Error as IoError;
use {Multiaddr, MuxedTransport, Transport};

/// Creates a swarm.
///
/// Requires an upgraded transport, and a function or closure that will turn the upgrade into a
/// `Future` that produces a `()`.
///
/// Produces a `SwarmController` and an implementation of `Future`. The controller can be used to
/// control, and the `Future` must be driven to completion in order for things to work.
///
pub fn swarm<T, H, F>(
    transport: T,
    handler: H,
) -> (SwarmController<T>, SwarmFuture<T, H, F::Future>)
where
    T: MuxedTransport + Clone + 'static, // TODO: 'static :-/
    H: FnMut(T::Output, Multiaddr) -> F,
    F: IntoFuture<Item = (), Error = IoError>,
{
    let (new_dialers_tx, new_dialers_rx) = mpsc::unbounded();
    let (new_listeners_tx, new_listeners_rx) = mpsc::unbounded();
    let (new_toprocess_tx, new_toprocess_rx) = mpsc::unbounded();

    let future = SwarmFuture {
        transport: transport.clone(),
        handler: handler,
        new_listeners: new_listeners_rx,
        next_incoming: transport.clone().next_incoming(),
        listeners: FuturesUnordered::new(),
        listeners_upgrade: FuturesUnordered::new(),
        dialers: FuturesUnordered::new(),
        new_dialers: new_dialers_rx,
        to_process: FuturesUnordered::new(),
        new_toprocess: new_toprocess_rx,
    };

    let controller = SwarmController {
        transport: transport,
        new_listeners: new_listeners_tx,
        new_dialers: new_dialers_tx,
        new_toprocess: new_toprocess_tx,
    };

    (controller, future)
}

/// Allows control of what the swarm is doing.
pub struct SwarmController<T>
where
    T: MuxedTransport + 'static, // TODO: 'static :-/
{
    transport: T,
    new_listeners: mpsc::UnboundedSender<T::Listener>,
    new_dialers: mpsc::UnboundedSender<Box<Future<Item = (T::Output, Multiaddr), Error = IoError>>>,
    new_toprocess: mpsc::UnboundedSender<Box<Future<Item = (), Error = IoError>>>,
}

impl<T> fmt::Debug for SwarmController<T>
where
    T: fmt::Debug + MuxedTransport + 'static, // TODO: 'static :-/
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_tuple("SwarmController")
            .field(&self.transport)
            .finish()
    }
}

impl<T> Clone for SwarmController<T>
where
    T: MuxedTransport + Clone + 'static, // TODO: 'static :-/
{
    fn clone(&self) -> SwarmController<T> {
        SwarmController {
            transport: self.transport.clone(),
            new_listeners: self.new_listeners.clone(),
            new_dialers: self.new_dialers.clone(),
            new_toprocess: self.new_toprocess.clone(),
        }
    }
}

impl<T> SwarmController<T>
where
    T: MuxedTransport + Clone + 'static, // TODO: 'static :-/
{
    /// Asks the swarm to dial the node with the given multiaddress. The connection is then
    /// upgraded using the `upgrade`, and the output is sent to the handler that was passed when
    /// calling `swarm`.
    // TODO: consider returning a future so that errors can be processed?
    pub fn dial_to_handler<Du>(&self, multiaddr: Multiaddr, transport: Du) -> Result<(), Multiaddr>
    where
        Du: Transport + 'static, // TODO: 'static :-/
        Du::Output: Into<T::Output>,
    {
        trace!(target: "libp2p-swarm", "Swarm dialing {}", multiaddr);

        match transport.dial(multiaddr.clone()) {
            Ok(dial) => {
                let dial = Box::new(
                    dial.into_future()
                        .map(|(d, client_addr)| (d.into(), client_addr)),
                ) as Box<Future<Item = _, Error = _>>;
                // Ignoring errors if the receiver has been closed, because in that situation
                // nothing is going to be processed anyway.
                let _ = self.new_dialers.unbounded_send(dial);
                Ok(())
            }
            Err((_, multiaddr)) => Err(multiaddr),
        }
    }

    /// Asks the swarm to dial the node with the given multiaddress. The connection is then
    /// upgraded using the `upgrade`, and the output is then passed to `and_then`.
    ///
    /// Contrary to `dial_to_handler`, the output of the upgrade is not given to the handler that
    /// was passed at initialization.
    // TODO: consider returning a future so that errors can be processed?
    pub fn dial_custom_handler<Du, Df, Dfu>(
        &self,
        multiaddr: Multiaddr,
        transport: Du,
        and_then: Df,
    ) -> Result<(), Multiaddr>
    where
        Du: Transport + 'static,                               // TODO: 'static :-/
        Df: FnOnce(Du::Output, Multiaddr) -> Dfu + 'static,    // TODO: 'static :-/
        Dfu: IntoFuture<Item = (), Error = IoError> + 'static, // TODO: 'static :-/
    {
        trace!(target: "libp2p-swarm", "Swarm dialing {} with custom handler", multiaddr);

        match transport.dial(multiaddr) {
            Ok(dial) => {
                let dial = Box::new(dial.into_future().and_then(|(d, m)| and_then(d, m))) as Box<_>;
                // Ignoring errors if the receiver has been closed, because in that situation
                // nothing is going to be processed anyway.
                let _ = self.new_toprocess.unbounded_send(dial);
                Ok(())
            }
            Err((_, multiaddr)) => Err(multiaddr),
        }
    }

    /// Adds a multiaddr to listen on. All the incoming connections will use the `upgrade` that
    /// was passed to `swarm`.
    pub fn listen_on(&self, multiaddr: Multiaddr) -> Result<Multiaddr, Multiaddr> {
        match self.transport.clone().listen_on(multiaddr) {
            Ok((listener, new_addr)) => {
                trace!(target: "libp2p-swarm", "Swarm listening on {}", new_addr);
                // Ignoring errors if the receiver has been closed, because in that situation
                // nothing is going to be processed anyway.
                let _ = self.new_listeners.unbounded_send(listener);
                Ok(new_addr)
            }
            Err((_, multiaddr)) => Err(multiaddr),
        }
    }
}

/// Future that must be driven to completion in order for the swarm to work.
pub struct SwarmFuture<T, H, F>
where
    T: MuxedTransport + 'static, // TODO: 'static :-/
{
    transport: T,
    handler: H,
    new_listeners: mpsc::UnboundedReceiver<T::Listener>,
    next_incoming: T::Incoming,
    listeners: FuturesUnordered<
        StreamFuture<
            Box<
                Stream<
                    Item = Box<Future<Item = (T::Output, Multiaddr), Error = IoError>>,
                    Error = IoError,
                >,
            >,
        >,
    >,
    listeners_upgrade:
        FuturesUnordered<Box<Future<Item = (T::Output, Multiaddr), Error = IoError>>>,
    dialers: FuturesUnordered<Box<Future<Item = (T::Output, Multiaddr), Error = IoError>>>,
    new_dialers:
        mpsc::UnboundedReceiver<Box<Future<Item = (T::Output, Multiaddr), Error = IoError>>>,
    to_process: FuturesUnordered<future::Either<F, Box<Future<Item = (), Error = IoError>>>>,
    new_toprocess: mpsc::UnboundedReceiver<Box<Future<Item = (), Error = IoError>>>,
}

impl<T, H, If, F> Future for SwarmFuture<T, H, F>
where
    T: MuxedTransport + Clone + 'static, // TODO: 'static :-/,
    H: FnMut(T::Output, Multiaddr) -> If,
    If: IntoFuture<Future = F, Item = (), Error = IoError>,
    F: Future<Item = (), Error = IoError>,
{
    type Item = ();
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let handler = &mut self.handler;

        match self.next_incoming.poll() {
            Ok(Async::Ready(connec)) => {
                debug!(target: "libp2p-swarm", "Swarm received new multiplexed \
                                                incoming connection");
                self.next_incoming = self.transport.clone().next_incoming();
                self.listeners_upgrade.push(Box::new(connec) as Box<_>);
            }
            Ok(Async::NotReady) => {}
            Err(err) => {
                debug!(target: "libp2p-swarm", "Error in multiplexed incoming \
                                                connection: {:?}", err);
                self.next_incoming = self.transport.clone().next_incoming();
            }
        };

        match self.new_listeners.poll() {
            Ok(Async::Ready(Some(new_listener))) => {
                let new_listener = Box::new(
                    new_listener.map(|f| Box::new(f) as Box<Future<Item = _, Error = _>>),
                ) as Box<Stream<Item = _, Error = _>>;
                self.listeners.push(new_listener.into_future());
            }
            Ok(Async::Ready(None)) | Err(_) => {
                // New listener sender has been closed.
            }
            Ok(Async::NotReady) => {}
        };

        match self.new_dialers.poll() {
            Ok(Async::Ready(Some(new_dialer))) => {
                self.dialers.push(new_dialer);
            }
            Ok(Async::Ready(None)) | Err(_) => {
                // New dialers sender has been closed.
            }
            Ok(Async::NotReady) => {}
        };

        match self.new_toprocess.poll() {
            Ok(Async::Ready(Some(new_toprocess))) => {
                self.to_process.push(future::Either::B(new_toprocess));
            }
            Ok(Async::Ready(None)) | Err(_) => {
                // New to-process sender has been closed.
            }
            Ok(Async::NotReady) => {}
        };

        match self.listeners.poll() {
            Ok(Async::Ready(Some((Some(upgrade), remaining)))) => {
                trace!(target: "libp2p-swarm", "Swarm received new connection on listener socket");
                self.listeners_upgrade.push(upgrade);
                self.listeners.push(remaining.into_future());
            }
            Err((err, _)) => {
                warn!(target: "libp2p-swarm", "Error in listener: {:?}", err);
            }
            _ => {}
        }

        match self.listeners_upgrade.poll() {
            Ok(Async::Ready(Some((output, client_addr)))) => {
                debug!(
                    "Successfully upgraded incoming connection with {}",
                    client_addr
                );
                self.to_process.push(future::Either::A(
                    handler(output, client_addr).into_future(),
                ));
            }
            Err(err) => {
                warn!(target: "libp2p-swarm", "Error in listener upgrade: {:?}", err);
            }
            _ => {}
        }

        match self.dialers.poll() {
            Ok(Async::Ready(Some((output, addr)))) => {
                trace!("Successfully upgraded dialed connection with {}", addr);
                self.to_process
                    .push(future::Either::A(handler(output, addr).into_future()));
            }
            Err(err) => {
                warn!(target: "libp2p-swarm", "Error in dialer upgrade: {:?}", err);
            }
            _ => {}
        }

        match self.to_process.poll() {
            Ok(Async::Ready(Some(()))) => {
                trace!(target: "libp2p-swarm", "Future returned by swarm handler driven to completion");
            }
            Err(err) => {
                warn!(target: "libp2p-swarm", "Error in processing: {:?}", err);
            }
            _ => {}
        }

        // TODO: we never return `Ok(Ready)` because there's no way to know whether
        //       `next_incoming()` can produce anything more in the future
        Ok(Async::NotReady)
    }
}
