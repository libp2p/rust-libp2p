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
//! extern crate libp2p_core;
//! extern crate libp2p_tcp_transport;
//! extern crate tokio_current_thread;
//!
//! use futures::{Future, Stream};
//! use libp2p_ping::{Ping, PingOutput};
//! use libp2p_core::Transport;
//!
//! # fn main() {
//! let ping_finished_future = libp2p_tcp_transport::TcpConfig::new()
//!     .with_upgrade(Ping::default())
//!     .dial("127.0.0.1:12345".parse::<libp2p_core::Multiaddr>().unwrap()).unwrap_or_else(|_| panic!())
//!     .and_then(|(out, _)| {
//!         match out {
//!             PingOutput::Ponger(processing) => Box::new(processing) as Box<Future<Item = _, Error = _> + Send>,
//!             PingOutput::Pinger(mut pinger) => {
//!                 pinger.ping(());
//!                 let f = pinger.into_future().map(|_| ()).map_err(|(err, _)| err);
//!                 Box::new(f) as Box<Future<Item = _, Error = _> + Send>
//!             },
//!         }
//!     });
//!
//! // Runs until the ping arrives.
//! tokio_current_thread::block_on_all(ping_finished_future).unwrap();
//! # }
//! ```
//!

extern crate bytes;
extern crate futures;
extern crate libp2p_core;
#[macro_use]
extern crate log;
extern crate multistream_select;
extern crate parking_lot;
extern crate rand;
extern crate tokio_codec;
extern crate tokio_io;

use bytes::{BufMut, Bytes, BytesMut};
use futures::{prelude::*, future::{FutureResult, IntoFuture}, task};
use libp2p_core::{ConnectionUpgrade, Endpoint};
use rand::{distributions::Standard, prelude::*, rngs::EntropyRng};
use std::collections::VecDeque;
use std::io::Error as IoError;
use std::{iter, marker::PhantomData, mem};
use tokio_codec::{Decoder, Encoder, Framed};
use tokio_io::{AsyncRead, AsyncWrite};

/// Represents a prototype for an upgrade to handle the ping protocol.
///
/// According to the design of libp2p, this struct would normally contain the configuration options
/// for the protocol, but in the case of `Ping` no configuration is required.
#[derive(Debug, Copy, Clone, Default)]
pub struct Ping<TUserData = ()>(PhantomData<TUserData>);

/// Output of a `Ping` upgrade.
pub enum PingOutput<TSocket, TUserData> {
    /// We are on the dialing side.
    Pinger(PingDialer<TSocket, TUserData>),
    /// We are on the listening side.
    Ponger(PingListener<TSocket>),
}

impl<TSocket, TUserData, Maf> ConnectionUpgrade<TSocket, Maf> for Ping<TUserData>
where
    TSocket: AsyncRead + AsyncWrite,
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once(("/ipfs/ping/1.0.0".into(), ()))
    }

    type Output = PingOutput<TSocket, TUserData>;
    type MultiaddrFuture = Maf;
    type Future = FutureResult<(Self::Output, Self::MultiaddrFuture), IoError>;

    #[inline]
    fn upgrade(
        self,
        socket: TSocket,
        _: Self::UpgradeIdentifier,
        endpoint: Endpoint,
        remote_addr: Maf,
    ) -> Self::Future {
        let out = match endpoint {
            Endpoint::Dialer => upgrade_as_dialer(socket),
            Endpoint::Listener => upgrade_as_listener(socket),
        };

        Ok((out, remote_addr)).into_future()
    }
}

/// Upgrades a connection from the dialer side.
fn upgrade_as_dialer<TSocket, TUserData>(socket: TSocket) -> PingOutput<TSocket, TUserData>
where TSocket: AsyncRead + AsyncWrite,
{
    let dialer = PingDialer {
        inner: Framed::new(socket, Codec),
        need_writer_flush: false,
        needs_close: false,
        sent_pings: VecDeque::with_capacity(4),
        rng: EntropyRng::default(),
        pings_to_send: VecDeque::with_capacity(4),
        to_notify: None,
    };

    PingOutput::Pinger(dialer)
}

/// Upgrades a connection from the listener side.
fn upgrade_as_listener<TSocket, TUserData>(socket: TSocket) -> PingOutput<TSocket, TUserData>
where TSocket: AsyncRead + AsyncWrite,
{
    let listener = PingListener {
        inner: Framed::new(socket, Codec),
        state: PingListenerState::Listening,
    };

    PingOutput::Ponger(listener)
}

/// Sends pings and receives the pongs.
///
/// Implements `Stream`. The stream indicates when we receive a pong.
pub struct PingDialer<TSocket, TUserData> {
    /// The underlying socket.
    inner: Framed<TSocket, Codec>,
    /// If true, need to flush the sink.
    need_writer_flush: bool,
    /// If true, need to close the sink.
    needs_close: bool,
    /// List of pings that have been sent to the remote and that are waiting for an answer.
    sent_pings: VecDeque<(Bytes, TUserData)>,
    /// Random number generator for the ping payload.
    rng: EntropyRng,
    /// List of pings to send to the remote.
    pings_to_send: VecDeque<(Bytes, TUserData)>,
    /// Task to notify when we add an element to `pings_to_send`.
    to_notify: Option<task::Task>,
}

impl<TSocket, TUserData> PingDialer<TSocket, TUserData> {
    /// Sends a ping to the remote.
    ///
    /// The stream will produce an event containing the user data when we receive the pong.
    pub fn ping(&mut self, user_data: TUserData) {
        let payload: [u8; 32] = self.rng.sample(Standard);
        debug!("Preparing for ping with payload {:?}", payload);
        self.pings_to_send.push_back((Bytes::from(payload.to_vec()), user_data));
        if let Some(to_notify) = self.to_notify.take() {
            to_notify.notify();
        }
    }
}

impl<TSocket, TUserData> Stream for PingDialer<TSocket, TUserData>
where TSocket: AsyncRead + AsyncWrite,
{
    type Item = TUserData;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        self.to_notify = Some(task::current());

        while let Some((ping, user_data)) = self.pings_to_send.pop_front() {
            match self.inner.start_send(ping.clone()) {
                Ok(AsyncSink::Ready) => self.need_writer_flush = true,
                Ok(AsyncSink::NotReady(_)) => {
                    self.pings_to_send.push_front((ping, user_data));
                    break;
                },
                Err(err) => return Err(err),
            }

            self.sent_pings.push_back((ping, user_data));
        }

        if self.need_writer_flush {
            match self.inner.poll_complete() {
                Ok(Async::Ready(())) => self.need_writer_flush = false,
                Ok(Async::NotReady) => (),
                Err(err) => return Err(err),
            }
        }

        loop {
            match self.inner.poll() {
                Ok(Async::Ready(Some(pong))) => {
                    if let Some(pos) = self.sent_pings.iter().position(|&(ref p, _)| p == &pong) {
                        let (_, user_data) = self.sent_pings.remove(pos)
                            .expect("Grabbed a valid position just above");
                        return Ok(Async::Ready(Some(user_data)));
                    } else {
                        debug!("Received pong that doesn't match what we went: {:?}", pong);
                    }
                },
                Ok(Async::NotReady) => break,
                Ok(Async::Ready(None)) => {
                    // Notify the current task so that we poll again.
                    self.needs_close = true;
                    task::current().notify();
                    return Ok(Async::NotReady);
                }
                Err(err) => return Err(err),
            }
        }

        Ok(Async::NotReady)
    }
}

/// Listens to incoming pings and answers them.
///
/// Implements `Future`. The future terminates when the underlying socket closes.
pub struct PingListener<TSocket> {
    /// The underlying socket.
    inner: Framed<TSocket, Codec>,
    /// State of the listener.
    state: PingListenerState,
}

#[derive(Debug)]
enum PingListenerState {
    /// We are waiting for the next ping on the socket.
    Listening,
    /// We are trying to send a pong.
    Sending(Bytes),
    /// We are flusing the underlying sink.
    Flushing,
    /// We are shutting down everything.
    Closing,
    /// A panic happened during the processing.
    Poisoned,
}

impl<TSocket> Future for PingListener<TSocket>
where TSocket: AsyncRead + AsyncWrite
{
    type Item = ();
    type Error = IoError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, PingListenerState::Poisoned) {
                PingListenerState::Listening => {
                    match self.inner.poll() {
                        Ok(Async::Ready(Some(payload))) => {
                            debug!("Received ping (payload={:?}) ; sending back", payload);
                            self.state = PingListenerState::Sending(payload.freeze())
                        },
                        Ok(Async::Ready(None)) => self.state = PingListenerState::Closing,
                        Ok(Async::NotReady) => {
                            self.state = PingListenerState::Listening;
                            return Ok(Async::NotReady);
                        },
                        Err(err) => return Err(err),
                    }
                },
                PingListenerState::Sending(data) => {
                    match self.inner.start_send(data) {
                        Ok(AsyncSink::Ready) => self.state = PingListenerState::Flushing,
                        Ok(AsyncSink::NotReady(data)) => {
                            self.state = PingListenerState::Sending(data);
                            return Ok(Async::NotReady);
                        },
                        Err(err) => return Err(err),
                    }
                },
                PingListenerState::Flushing => {
                    match self.inner.poll_complete() {
                        Ok(Async::Ready(())) => self.state = PingListenerState::Listening,
                        Ok(Async::NotReady) => {
                            self.state = PingListenerState::Flushing;
                            return Ok(Async::NotReady);
                        },
                        Err(err) => return Err(err),
                    }
                },
                PingListenerState::Closing => {
                    match self.inner.close() {
                        Ok(Async::Ready(())) => return Ok(Async::Ready(())),
                        Ok(Async::NotReady) => {
                            self.state = PingListenerState::Closing;
                            return Ok(Async::NotReady);
                        },
                        Err(err) => return Err(err),
                    }
                },
                PingListenerState::Poisoned => panic!("Poisoned or errored PingListener"),
            }
        }
    }
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
        if !data.is_empty() {
            let split = 32 * (1 + ((data.len() - 1) / 32));
            buf.reserve(split);
            buf.put(data.split_to(split));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate tokio_current_thread;
    extern crate tokio_tcp;

    use self::tokio_tcp::TcpListener;
    use self::tokio_tcp::TcpStream;
    use super::{Ping, PingOutput};
    use futures::{future, Future, Stream};
    use libp2p_core::{ConnectionUpgrade, Endpoint, Multiaddr};
    use std::io::Error as IoError;

    // TODO: rewrite tests with the MemoryTransport

    #[test]
    fn ping_pong() {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map_err(|(e, _)| e.into())
            .and_then(|(c, _)| {
                Ping::<()>::default().upgrade(
                    c.unwrap(),
                    (),
                    Endpoint::Listener,
                    future::ok::<Multiaddr, IoError>("/ip4/127.0.0.1/tcp/10000".parse().unwrap()),
                )
            })
            .and_then(|(out, _)| match out {
                PingOutput::Ponger(service) => service,
                _ => unreachable!(),
            });

        let client = TcpStream::connect(&listener_addr)
            .map_err(|e| e.into())
            .and_then(|c| {
                Ping::<()>::default().upgrade(
                    c,
                    (),
                    Endpoint::Dialer,
                    future::ok::<Multiaddr, IoError>("/ip4/127.0.0.1/tcp/10000".parse().unwrap()),
                )
            })
            .and_then(|(out, _)| match out {
                PingOutput::Pinger(mut pinger) => {
                    pinger.ping(());
                    pinger.into_future().map(|_| ()).map_err(|_| panic!())
                },
                _ => unreachable!(),
            })
            .map(|_| ());

        tokio_current_thread::block_on_all(server.select(client).map_err(|_| panic!())).unwrap();
    }

    #[test]
    fn multipings() {
        // Check that we can send multiple pings in a row and it will still work.
        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map_err(|(e, _)| e.into())
            .and_then(|(c, _)| {
                Ping::<u32>::default().upgrade(
                    c.unwrap(),
                    (),
                    Endpoint::Listener,
                    future::ok::<Multiaddr, IoError>("/ip4/127.0.0.1/tcp/10000".parse().unwrap()),
                )
            })
            .and_then(|(out, _)| match out {
                PingOutput::Ponger(service) => service,
                _ => unreachable!(),
            });

        let client = TcpStream::connect(&listener_addr)
            .map_err(|e| e.into())
            .and_then(|c| {
                Ping::<u32>::default().upgrade(
                    c,
                    (),
                    Endpoint::Dialer,
                    future::ok::<Multiaddr, IoError>("/ip4/127.0.0.1/tcp/10000".parse().unwrap()),
                )
            })
            .and_then(|(out, _)| match out {
                PingOutput::Pinger(mut pinger) => {
                    for n in 0..20 {
                        pinger.ping(n);
                    }

                    pinger
                        .take(20)
                        .collect()
                        .map(|val| { assert_eq!(val, (0..20).collect::<Vec<_>>()); })
                        .map_err(|_| panic!())
                },
                _ => unreachable!(),
            });

        tokio_current_thread::block_on_all(server.select(client)).unwrap_or_else(|_| panic!());
    }
}
