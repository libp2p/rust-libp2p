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

use bytes::{BufMut, Bytes, BytesMut};
use futures::{prelude::*, future::{self, FutureResult}, try_ready};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use log::debug;
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
#[derive(Debug, Copy, Clone)]
pub struct Ping<TUserData = ()>(PhantomData<TUserData>);

impl<TUserData> Default for Ping<TUserData> {
    #[inline]
    fn default() -> Self {
        Ping(PhantomData)
    }
}

impl<TUserData> UpgradeInfo for Ping<TUserData> {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/ipfs/ping/1.0.0")
    }
}

impl<TSocket, TUserData> InboundUpgrade<TSocket> for Ping<TUserData>
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = PingListener<TSocket>;
    type Error = IoError;
    type Future = FutureResult<Self::Output, Self::Error>;

    #[inline]
    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let listener = PingListener {
            inner: Framed::new(socket, Codec),
            state: PingListenerState::Listening,
        };
        future::ok(listener)
    }
}

impl<TSocket, TUserData> OutboundUpgrade<TSocket> for Ping<TUserData>
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = PingDialer<TSocket, TUserData>;
    type Error = IoError;
    type Future = FutureResult<Self::Output, Self::Error>;

    #[inline]
    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let dialer = PingDialer {
            inner: Framed::new(socket, Codec),
            need_writer_flush: false,
            needs_close: false,
            sent_pings: VecDeque::with_capacity(4),
            rng: EntropyRng::default(),
            pings_to_send: VecDeque::with_capacity(4),
        };
        future::ok(dialer)
    }
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
}

impl<TSocket, TUserData> PingDialer<TSocket, TUserData> {
    /// Sends a ping to the remote.
    ///
    /// The stream will produce an event containing the user data when we receive the pong.
    pub fn ping(&mut self, user_data: TUserData) {
        let payload: [u8; 32] = self.rng.sample(Standard);
        debug!("Preparing for ping with payload {:?}", payload);
        self.pings_to_send.push_back((Bytes::from(payload.to_vec()), user_data));
    }
}

impl<TSocket, TUserData> PingDialer<TSocket, TUserData>
where TSocket: AsyncRead + AsyncWrite,
{
    /// Call this when the ping dialer needs to shut down. After this, the `Stream` is guaranteed
    /// to finish soon-ish.
    #[inline]
    pub fn shutdown(&mut self) {
        self.needs_close = true;
    }
}

impl<TSocket, TUserData> Stream for PingDialer<TSocket, TUserData>
where TSocket: AsyncRead + AsyncWrite,
{
    type Item = TUserData;
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.needs_close {
            try_ready!(self.inner.close());
            return Ok(Async::Ready(None));
        }

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
                        debug!("Received pong that doesn't match what we sent: {:?}", pong);
                    }
                },
                Ok(Async::NotReady) => break,
                Ok(Async::Ready(None)) => {
                    // Notify the current task so that we poll again.
                    self.needs_close = true;
                    try_ready!(self.inner.close());
                    return Ok(Async::Ready(None));
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
    /// We are flushing the underlying sink.
    Flushing,
    /// We are shutting down everything.
    Closing,
    /// A panic happened during the processing.
    Poisoned,
}

impl<TSocket> PingListener<TSocket>
where TSocket: AsyncRead + AsyncWrite
{
    /// Call this when the ping listener needs to shut down. After this, the `Future` is guaranteed
    /// to finish soon-ish.
    #[inline]
    pub fn shutdown(&mut self) {
        self.state = PingListenerState::Closing;
    }
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
    use tokio_tcp::{TcpListener, TcpStream};
    use super::Ping;
    use futures::{Future, Stream};
    use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade};

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
                Ping::<()>::default().upgrade_inbound(c.unwrap(), b"/ipfs/ping/1.0.0")
            })
            .flatten();

        let client = TcpStream::connect(&listener_addr)
            .map_err(|e| e.into())
            .and_then(|c| {
                Ping::<()>::default().upgrade_outbound(c, b"/ipfs/ping/1.0.0")
            })
            .and_then(|mut pinger| {
                pinger.ping(());
                pinger.into_future().map(|_| ()).map_err(|_| panic!())
            })
            .map(|_| ());

        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(server.select(client).map_err(|_| panic!())).unwrap();
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
                Ping::<u32>::default().upgrade_inbound(c.unwrap(), b"/ipfs/ping/1.0.0")
            })
            .flatten();

        let client = TcpStream::connect(&listener_addr)
            .map_err(|e| e.into())
            .and_then(|c| {
                Ping::<u32>::default().upgrade_outbound(c, b"/ipfs/ping/1.0.0")
            })
            .and_then(|mut pinger| {
                for n in 0..20 {
                    pinger.ping(n);
                }
                pinger.take(20)
                    .collect()
                    .map(|val| { assert_eq!(val, (0..20).collect::<Vec<_>>()); })
                    .map_err(|_| panic!())
            });

        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(server.select(client)).unwrap_or_else(|_| panic!());
    }
}
