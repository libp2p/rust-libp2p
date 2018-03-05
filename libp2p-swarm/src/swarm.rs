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

use std::io::Error as IoError;
use futures::{IntoFuture, Future, Stream, Async, Poll, future};
use futures::sync::mpsc;
use {ConnectionUpgrade, Multiaddr, MuxedTransport, UpgradedNode};

/// Creates a swarm.
///
/// Requires an upgraded transport, and a function or closure that will turn the upgrade into a
/// `Future` that produces a `()`.
///
/// Produces a `SwarmController` and an implementation of `Future`. The controller can be used to
/// control, and the `Future` must be driven to completion in order for things to work.
///
pub fn swarm<T, C, H, F, Conf>(transport: T, upgrade: C, handler: H, conf: Conf)
                         -> (SwarmController<T, C, Conf>, SwarmFuture<T, C, H, F::Future, Conf>)
where
    T: MuxedTransport<Conf> + Clone + 'static,      // TODO: 'static :-/
    C: ConnectionUpgrade<T::RawConn, Conf> + Clone + 'static,      // TODO: 'static :-/
    C::NamesIter: Clone,      // TODO: not elegant
    H: FnMut(C::Output, Multiaddr) -> F,
    F: IntoFuture<Item = (), Error = IoError>,
    Conf: Clone,
{
    let (new_dialers_tx, new_dialers_rx) = mpsc::unbounded();
    let (new_listeners_tx, new_listeners_rx) = mpsc::unbounded();
    let (new_toprocess_tx, new_toprocess_rx) = mpsc::unbounded();

    let upgraded = transport.clone().with_upgrade(upgrade);

    let future = SwarmFuture {
        upgraded: upgraded.clone(),
        config: conf.clone(),
        handler: handler,
        new_listeners: new_listeners_rx,
        next_incoming: upgraded.clone().next_incoming(conf),
        listeners: Vec::new(),
        listeners_upgrade: Vec::new(),
        dialers: Vec::new(),
        new_dialers: new_dialers_rx,
        to_process: Vec::new(),
        new_toprocess: new_toprocess_rx,
    };

    let controller = SwarmController {
        transport: transport,
        upgraded: upgraded,
        config: conf,
        new_listeners: new_listeners_tx,
        new_dialers: new_dialers_tx,
        new_toprocess: new_toprocess_tx,
    };

    (controller, future)
}

/// Allows control of what the swarm is doing.
pub struct SwarmController<T, C, Conf>
    where T: MuxedTransport<Conf> + 'static,      // TODO: 'static :-/
          C: ConnectionUpgrade<T::RawConn, Conf> + 'static,      // TODO: 'static :-/
{
    transport: T,
    upgraded: UpgradedNode<T, C, Conf>,
    config: Conf,
    new_listeners: mpsc::UnboundedSender<Box<Stream<Item = Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>, Error = IoError>>>,
    new_dialers: mpsc::UnboundedSender<Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>>,
    new_toprocess: mpsc::UnboundedSender<Box<Future<Item = (), Error = IoError>>>,
}

impl<T: 'static, C: 'static, Conf: 'static> SwarmController<T, C, Conf>
where
    T: MuxedTransport<Conf> + Clone,
    C: ConnectionUpgrade<T::RawConn, Conf> + Clone,
    C::NamesIter: Clone, // TODO: not elegant
    Conf: Clone, // Should be a cheaply-cloneable type like &_
{
    /// Asks the swarm to dial the node with the given multiaddress. The connection is then
    /// upgraded using the `upgrade`, and the output is sent to the handler that was passed when
    /// calling `swarm`.
    // TODO: consider returning a future so that errors can be processed?
    pub fn dial_to_handler<Du>(&self, multiaddr: Multiaddr, upgrade: Du) -> Result<(), Multiaddr>
        where Du: ConnectionUpgrade<T::RawConn, Conf> + Clone + 'static,      // TODO: 'static :-/
              Du::Output: Into<C::Output>,
    {
        match self.transport.clone().with_upgrade(upgrade).dial(multiaddr.clone(), self.config.clone()) {
            Ok(dial) => {
                let dial = Box::new(dial.map(|(d, client_addr)| (d.into(), client_addr))) as Box<Future<Item = _, Error = _>>;
                // Ignoring errors if the receiver has been closed, because in that situation
                // nothing is going to be processed anyway.
                let _ = self.new_dialers.unbounded_send(dial);
                Ok(())
            },
            Err((_, multiaddr)) => {
                Err(multiaddr)
            },
        }
    }

    /// Asks the swarm to dial the node with the given multiaddress. The connection is then
    /// upgraded using the `upgrade`, and the output is then passed to `and_then`.
    ///
    /// Contrary to `dial_to_handler`, the output of the upgrade is not given to the handler that
    /// was passed at initialization.
    // TODO: consider returning a future so that errors can be processed?
    pub fn dial_custom_handler<Du, Df, Dfu>(&self, multiaddr: Multiaddr, upgrade: Du, and_then: Df)
                                            -> Result<(), Multiaddr>
        where Du: ConnectionUpgrade<T::RawConn, Conf> + 'static,      // TODO: 'static :-/
              Df: FnOnce(Du::Output, Multiaddr) -> Dfu + 'static,          // TODO: 'static :-/
              Dfu: IntoFuture<Item = (), Error = IoError> + 'static,        // TODO: 'static :-/
    {
        match self.transport.clone().with_upgrade(upgrade).dial(multiaddr, self.config.clone()) {
            Ok(dial) => {
                let dial = Box::new(dial.and_then(|(d, m)| and_then(d, m))) as Box<_>;
                // Ignoring errors if the receiver has been closed, because in that situation
                // nothing is going to be processed anyway.
                let _ = self.new_toprocess.unbounded_send(dial);
                Ok(())
            },
            Err((_, multiaddr)) => {
                Err(multiaddr)
            },
        }
    }

    /// Adds a multiaddr to listen on. All the incoming connections will use the `upgrade` that
    /// was passed to `swarm`.
    pub fn listen_on(&self, multiaddr: Multiaddr, conf: Conf) -> Result<Multiaddr, Multiaddr> {
        match self.upgraded.clone().listen_on(multiaddr, conf) {
            Ok((listener, new_addr)) => {
                // Ignoring errors if the receiver has been closed, because in that situation
                // nothing is going to be processed anyway.
                let _ = self.new_listeners.unbounded_send(listener);
                Ok(new_addr)
            },
            Err((_, multiaddr)) => {
                Err(multiaddr)
            },
        }
    }
}

// TODO: 'static :-/
/// Future that must be driven to completion in order for the swarm to work.
pub struct SwarmFuture<T: 'static, C: 'static, H, F, Conf: 'static>
    where T: MuxedTransport<Conf>,
          C: ConnectionUpgrade<T::RawConn, Conf>,
{
    upgraded: UpgradedNode<T, C, Conf>,
    handler: H,
    config: Conf,
    new_listeners: mpsc::UnboundedReceiver<Box<Stream<Item = Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>, Error = IoError>>>,
    next_incoming: Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>,
    listeners: Vec<Box<Stream<Item = Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>, Error = IoError>>>,
    listeners_upgrade: Vec<Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>>,
    dialers: Vec<Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>>,
    new_dialers: mpsc::UnboundedReceiver<Box<Future<Item = (C::Output, Multiaddr), Error = IoError>>>,
    to_process: Vec<future::Either<F, Box<Future<Item = (), Error = IoError>>>>,
    new_toprocess: mpsc::UnboundedReceiver<Box<Future<Item = (), Error = IoError>>>,
}

impl<Trans, Upgrade, Handler, IntoFut, Conf> Future for
    SwarmFuture<Trans, Upgrade, Handler, IntoFut::Future, Conf>
where
    Trans: MuxedTransport<Conf> + Clone + 'static,      // TODO: 'static :-/,
    Upgrade: ConnectionUpgrade<Trans::RawConn, Conf> + Clone + 'static,      // TODO: 'static :-/
    Upgrade::NamesIter: Clone,      // TODO: not elegant
    Handler: FnMut(Upgrade::Output, Multiaddr) -> IntoFut,
    IntoFut: IntoFuture<Item = (), Error = IoError>,
    Conf: Clone,
{
    type Item = ();
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let handler = &mut self.handler;

        match self.next_incoming.poll() {
            Ok(Async::Ready((connec, client_addr))) => {
                self.next_incoming = self.upgraded.clone().next_incoming(self.config.clone());
                self.to_process.push(future::Either::A(handler(connec, client_addr).into_future()));
            },
            Ok(Async::NotReady) => {},
            // TODO: may not be the best idea because we're killing the whole server
            Err(err) => return Err(err),
        };

        match self.new_listeners.poll() {
            Ok(Async::Ready(Some(new_listener))) => {
                self.listeners.push(new_listener);
            },
            Ok(Async::Ready(None)) | Err(_) => {
                // New listener sender has been closed.
            },
            Ok(Async::NotReady) => {},
        };

        match self.new_dialers.poll() {
            Ok(Async::Ready(Some(new_dialer))) => {
                self.dialers.push(new_dialer);
            },
            Ok(Async::Ready(None)) | Err(_) => {
                // New dialers sender has been closed.
            },
            Ok(Async::NotReady) => {},
        };

        match self.new_toprocess.poll() {
            Ok(Async::Ready(Some(new_toprocess))) => {
                self.to_process.push(future::Either::B(new_toprocess));
            },
            Ok(Async::Ready(None)) | Err(_) => {
                // New to-process sender has been closed.
            },
            Ok(Async::NotReady) => {},
        };

        for n in (0 .. self.listeners.len()).rev() {
            let mut listener = self.listeners.swap_remove(n);
            match listener.poll() {
                Ok(Async::Ready(Some(upgrade))) => {
                    self.listeners.push(listener);
                    self.listeners_upgrade.push(upgrade);
                },
                Ok(Async::NotReady) => {
                    self.listeners.push(listener);
                },
                Ok(Async::Ready(None)) => {},
                Err(err) => return Err(err),
            };
        }

        for n in (0 .. self.listeners_upgrade.len()).rev() {
            let mut upgrade = self.listeners_upgrade.swap_remove(n);
            match upgrade.poll() {
                Ok(Async::Ready((output, client_addr))) => {
                    self.to_process.push(future::Either::A(handler(output, client_addr).into_future()));
                },
                Ok(Async::NotReady) => {
                    self.listeners_upgrade.push(upgrade);
                },
                Err(err) => return Err(err),
            }
        }

        for n in (0 .. self.dialers.len()).rev() {
            let mut dialer = self.dialers.swap_remove(n);
            match dialer.poll() {
                Ok(Async::Ready((output, addr))) => {
                    self.to_process.push(future::Either::A(handler(output, addr).into_future()));
                },
                Ok(Async::NotReady) => {
                    self.dialers.push(dialer);
                },
                Err(err) => return Err(err),
            }
        }

        for n in (0 .. self.to_process.len()).rev() {
            let mut to_process = self.to_process.swap_remove(n);
            match to_process.poll() {
                Ok(Async::Ready(())) => {},
                Ok(Async::NotReady) => self.to_process.push(to_process),
                Err(err) => return Err(err),
            }
        }

        // TODO: we never return `Ok(Ready)` because there's no way to know whether
        //       `next_incoming()` can produce anything more in the future
        Ok(Async::NotReady)
    }
}
