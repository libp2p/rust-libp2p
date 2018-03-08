// Copyright 2017 Parity Technologies (UK) Ltd.
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

// TODO: use this once stable ; for now we just copy-paste the content of the README.md
//#![doc(include = "../README.md")]

//! Handles the `/ipfs/ping/1.0.0` protocol. This allows pinging a remote node and waiting for an
//! answer.
//!
//! # Usage
//!
//! Create a `Ping` struct, which implements the `ConnectionUpgrade` trait. When used as a
//! connection upgrade, it will produce a tuple of type `(Pinger, impl Future<Item = ()>)` which
//! are named the *pinger* and the *ponger*.
//!
//! The *pinger* has a method named `ping` which will send a ping to the remote, while the *ponger*
//! is a future that will process the data received on the socket and will be signalled only when
//! the connection closes.
//!
//! # About timeouts
//!
//! For technical reasons, this crate doesn't handle timeouts. The action of pinging returns a
//! future that is signalled only when the remote answers. If the remote is not responsive, the
//! future will never be signalled.
//!
//! For implementation reasons, resources allocated for a ping are only ever fully reclaimed after
//! a pong has been received by the remote. Therefore if you repeatidely ping a non-responsive
//! remote you will end up using more and memory memory (albeit the amount is very very small every
//! time), even if you destroy the future returned by `ping`.
//!
//! This is probably not a problem in practice, because the nature of the ping protocol is to
//! determine whether a remote is still alive, and any reasonable user of this crate will close
//! connections to non-responsive remotes.
//!
//! # Example
//!
//! ```no_run
//! extern crate futures;
//! extern crate libp2p_ping;
//! extern crate libp2p_swarm;
//! extern crate libp2p_tcp_transport;
//! extern crate tokio_core;
//!
//! use futures::Future;
//! use libp2p_ping::Ping;
//! use libp2p_swarm::Transport;
//!
//! # fn main() {
//! let mut core = tokio_core::reactor::Core::new().unwrap();
//!
//! let ping_finished_future = libp2p_tcp_transport::TcpConfig::new(core.handle())
//!     .with_upgrade(Ping)
//!     .dial("127.0.0.1:12345".parse::<libp2p_swarm::Multiaddr>().unwrap()).unwrap_or_else(|_| panic!())
//!     .and_then(|((mut pinger, service), _)| {
//!         pinger.ping().map_err(|_| panic!()).select(service).map_err(|_| panic!())
//!     });
//!
//! // Runs until the ping arrives.
//! core.run(ping_finished_future).unwrap();
//! # }
//! ```
//!

extern crate bytes;
extern crate futures;
extern crate libp2p_swarm;
#[macro_use]
extern crate log;
extern crate multistream_select;
extern crate parking_lot;
extern crate rand;
extern crate tokio_io;

use bytes::{BufMut, Bytes, BytesMut};
use futures::{Future, Sink, Stream};
use futures::future::{loop_fn, FutureResult, IntoFuture, Loop};
use futures::sync::{mpsc, oneshot};
use libp2p_swarm::Multiaddr;
use libp2p_swarm::transport::{ConnectionUpgrade, Endpoint};
use log::Level;
use parking_lot::Mutex;
use rand::Rand;
use rand::os::OsRng;
use std::collections::HashMap;
use std::error::Error;
use std::io::Error as IoError;
use std::iter;
use std::sync::Arc;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::{Decoder, Encoder};

/// Represents a prototype for an upgrade to handle the ping protocol.
///
/// According to the design of libp2p, this struct would normally contain the configuration options
/// for the protocol, but in the case of `Ping` no configuration is required.
#[derive(Debug, Copy, Clone, Default)]
pub struct Ping;

impl<C, Conf> ConnectionUpgrade<C, Conf> for Ping
where
    C: AsyncRead + AsyncWrite + 'static,
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once(("/ipfs/ping/1.0.0".into(), ()))
    }

    type Output = (Pinger, Box<Future<Item = (), Error = IoError>>);
    type Future = FutureResult<Self::Output, IoError>;

    #[inline]
    fn upgrade(
        self,
        socket: C,
        _: Self::UpgradeIdentifier,
        _: Endpoint,
        remote_addr: &Multiaddr,
        _: &Multiaddr,
        _: Conf,
    ) -> Self::Future {
        // # How does it work?
        //
        // All the actual processing is performed by the *ponger*.
        // We use a channel in order to send ping requests from the pinger to the ponger.

        let (tx, rx) = mpsc::channel(8);
        // Ignore the errors if `tx` closed. `tx` is only ever closed if the ponger is closed,
        // which means that the connection to the remote is closed. Therefore we make the `rx`
        // never produce anything.
        let rx = rx.then(|r| Ok(r.ok())).filter_map(|a| a);

        let os_rng = match OsRng::new() {
            Ok(r) => r,
            Err(err) => return Err(err).into_future(),
        };

        let pinger = Pinger {
            send: tx,
            os_rng: os_rng,
        };

        // Hashmap that associates outgoing payloads to one-shot senders.
        // TODO: can't figure out how to make it work without using an Arc/Mutex
        let expected_pongs = Arc::new(Mutex::new(HashMap::with_capacity(4)));

        let sink_stream = socket
            .framed(Codec)
            .map(|msg| Message::Received(msg.freeze()));
        let (sink, stream) = sink_stream.split();

        let remote_addr = if log_enabled!(target: "libp2p-ping", Level::Debug) {
            Some(remote_addr.clone())
        } else {
            None
        };

        let future = loop_fn((sink, stream.select(rx)), move |(sink, stream)| {
            let expected_pongs = expected_pongs.clone();
            let remote_addr = remote_addr.clone();

            stream
                .into_future()
                .map_err(|(err, _)| err)
                .and_then(move |(message, stream)| {
                    let mut expected_pongs = expected_pongs.lock();

                    if let Some(message) = message {
                        match message {
                            Message::Ping(payload, finished) => {
                                // Ping requested by the user through the `Pinger`.
                                debug!(target: "libp2p-ping", "Sending ping to {:?} with payload {:?}",
                                   remote_addr.expect("debug log level is enabled"), payload);
                                expected_pongs.insert(payload.clone(), finished);
                                Box::new(
                                    sink.send(payload)
                                        .map(|sink| Loop::Continue((sink, stream))),
                                )
                                    as Box<Future<Item = _, Error = _>>
                            }
                            Message::Received(payload) => {
                                // Received a payload from the remote.
                                if let Some(fut) = expected_pongs.remove(&payload) {
                                    // Payload was ours. Signalling future.
                                    // Errors can happen if the user closed the receiving end of
                                    // the future, which is fine to ignore.
                                    debug!(target: "libp2p-ping", "Received pong from {:?} \
                                                               (payload={:?}) ; ping fufilled",
                                       remote_addr.expect("debug log level is enabled"), payload);
                                    let _ = fut.send(());
                                    Box::new(Ok(Loop::Continue((sink, stream))).into_future())
                                        as Box<Future<Item = _, Error = _>>
                                } else {
                                    // Payload was not ours. Sending it back.
                                    debug!(target: "libp2p-ping", "Received ping from {:?} \
                                                               (payload={:?}) ; sending back",
                                       remote_addr.expect("debug log level is enabled"), payload);
                                    Box::new(
                                        sink.send(payload)
                                            .map(|sink| Loop::Continue((sink, stream))),
                                    )
                                        as Box<Future<Item = _, Error = _>>
                                }
                            }
                        }
                    } else {
                        Box::new(Ok(Loop::Break(())).into_future())
                            as Box<Future<Item = _, Error = _>>
                    }
                })
        });

        Ok((pinger, Box::new(future) as Box<_>)).into_future()
    }
}

/// Controller for the ping service. Makes it possible to send pings to the remote.
pub struct Pinger {
    send: mpsc::Sender<Message>,
    os_rng: OsRng,
}

impl Pinger {
    /// Sends a ping. Returns a future that is signaled when a pong is received.
    ///
    /// **Note**: Please be aware that there is no timeout on the ping. You should handle the
    ///           timeout yourself when you call this function.
    pub fn ping(&mut self) -> Box<Future<Item = (), Error = Box<Error + Send + Sync>>> {
        let (tx, rx) = oneshot::channel();
        let payload: [u8; 32] = Rand::rand(&mut self.os_rng);
        debug!(target: "libp2p-ping", "Preparing for ping with payload {:?}", payload);
        // Ignore errors if the ponger has been already destroyed. The returned future will never
        // be signalled.
        let fut = self.send
            .clone()
            .send(Message::Ping(Bytes::from(payload.to_vec()), tx))
            .from_err()
            .and_then(|_| rx.from_err());
        Box::new(fut) as Box<_>
    }
}

enum Message {
    Ping(Bytes, oneshot::Sender<()>),
    Received(Bytes),
}

// Implementation of the `Codec` trait of tokio-io. Splits frames into groups of 32 bytes.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
struct Codec;

impl Decoder for Codec {
    type Item = BytesMut;
    type Error = IoError;

    #[inline]
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<BytesMut>, IoError> {
        if buf.len() >= 32 {
            Ok(Some(buf.split_to(32)))
        } else {
            Ok(None)
        }
    }
}

impl Encoder for Codec {
    type Item = Bytes;
    type Error = IoError;

    #[inline]
    fn encode(&mut self, mut data: Bytes, buf: &mut BytesMut) -> Result<(), IoError> {
        if data.len() != 0 {
            let split = 32 * (1 + ((data.len() - 1) / 32));
            buf.put(data.split_to(split));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate tokio_core;

    use self::tokio_core::net::TcpListener;
    use self::tokio_core::net::TcpStream;
    use self::tokio_core::reactor::Core;
    use super::Ping;
    use futures::future::join_all;
    use futures::Future;
    use futures::Stream;
    use libp2p_swarm::transport::{ConnectionUpgrade, Endpoint};

    #[test]
    fn ping_pong() {
        let mut core = Core::new().unwrap();

        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map_err(|(e, _)| e.into())
            .and_then(|(c, _)| {
                Ping.upgrade(
                    c.unwrap().0,
                    (),
                    Endpoint::Listener,
                    &"/ip4/127.0.0.1/tcp/10000".parse().unwrap(),
                    (),
                )
            })
            .and_then(|(mut pinger, service)| {
                pinger
                    .ping()
                    .map_err(|_| panic!())
                    .select(service)
                    .map_err(|_| panic!())
            });

        let client = TcpStream::connect(&listener_addr, &core.handle())
            .map_err(|e| e.into())
            .and_then(|c| {
                Ping.upgrade(
                    c,
                    (),
                    Endpoint::Dialer,
                    &"/ip4/127.0.0.1/tcp/10000".parse().unwrap(),
                    (),
                )
            })
            .and_then(|(mut pinger, service)| {
                pinger
                    .ping()
                    .map_err(|_| panic!())
                    .select(service)
                    .map_err(|_| panic!())
            });

        core.run(server.join(client)).unwrap();
    }

    #[test]
    fn multipings() {
        // Check that we can send multiple pings in a row and it will still work.
        let mut core = Core::new().unwrap();

        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map_err(|(e, _)| e.into())
            .and_then(|(c, _)| {
                Ping.upgrade(
                    c.unwrap().0,
                    (),
                    Endpoint::Listener,
                    &"/ip4/127.0.0.1/tcp/10000".parse().unwrap(),
                    (),
                )
            })
            .and_then(|(_, service)| service.map_err(|_| panic!()));

        let client = TcpStream::connect(&listener_addr, &core.handle())
            .map_err(|e| e.into())
            .and_then(|c| {
                Ping.upgrade(
                    c,
                    (),
                    Endpoint::Dialer,
                    &"/ip4/127.0.0.1/tcp/1000".parse().unwrap(),
                    (),
                )
            })
            .and_then(|(mut pinger, service)| {
                let pings = (0..20).map(move |_| pinger.ping().map_err(|_| ()));

                join_all(pings)
                    .map(|_| ())
                    .map_err(|_| panic!())
                    .select(service)
                    .map(|_| ())
                    .map_err(|_| panic!())
            });

        core.run(server.select(client)).unwrap_or_else(|_| panic!());
    }
}
