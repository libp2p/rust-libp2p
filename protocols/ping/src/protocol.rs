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

use futures::{prelude::*, future, try_ready};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use log::debug;
use rand::{distributions::Standard, prelude::*, rngs::EntropyRng};
use std::{io, iter, time::Duration, time::Instant};
use tokio_io::{AsyncRead, AsyncWrite};

/// Represents a prototype for an upgrade to handle the ping protocol.
///
/// The protocol works the following way:
///
/// - Dialer sends 32 bytes of random data.
/// - Listener receives the data and sends it back.
/// - Dialer receives the data and verifies that it matches what it sent.
///
/// The dialer produces a `Duration`, which corresponds to the time between when we flushed the
/// substream and when we received back the payload.
#[derive(Default, Debug, Copy, Clone)]
pub struct Ping;

impl UpgradeInfo for Ping {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/ipfs/ping/1.0.0")
    }
}

impl<TSocket> InboundUpgrade<TSocket> for Ping
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = ();
    type Error = io::Error;
    type Future = future::Map<future::AndThen<future::AndThen<future::AndThen<tokio_io::io::ReadExact<TSocket, [u8; 32]>, tokio_io::io::WriteAll<TSocket, [u8; 32]>, fn((TSocket, [u8; 32])) -> tokio_io::io::WriteAll<TSocket, [u8; 32]>>, tokio_io::io::Flush<TSocket>, fn((TSocket, [u8; 32])) -> tokio_io::io::Flush<TSocket>>, tokio_io::io::Shutdown<TSocket>, fn(TSocket) -> tokio_io::io::Shutdown<TSocket>>, fn(TSocket) -> ()>;

    #[inline]
    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        tokio_io::io::read_exact(socket, [0; 32])
            .and_then::<fn(_) -> _, _>(|(socket, buffer)| tokio_io::io::write_all(socket, buffer))
            .and_then::<fn(_) -> _, _>(|(socket, _)| tokio_io::io::flush(socket))
            .and_then::<fn(_) -> _, _>(|socket| tokio_io::io::shutdown(socket))
            .map(|_| ())
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for Ping
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = Duration;
    type Error = io::Error;
    type Future = PingDialer<TSocket>;

    #[inline]
    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let payload: [u8; 32] = EntropyRng::default().sample(Standard);
        debug!("Preparing for ping with payload {:?}", payload);

        PingDialer {
            inner: PingDialerInner::Write {
                inner: tokio_io::io::write_all(socket, payload),
            },
        }
    }
}

/// Sends a ping and receives a pong.
///
/// Implements `Future`. Finishes when the pong has arrived and has been verified.
pub struct PingDialer<TSocket> {
    inner: PingDialerInner<TSocket>,
}

enum PingDialerInner<TSocket> {
    Write {
        inner: tokio_io::io::WriteAll<TSocket, [u8; 32]>,
    },
    Flush {
        inner: tokio_io::io::Flush<TSocket>,
        ping_payload: [u8; 32],
    },
    Read {
        inner: tokio_io::io::ReadExact<TSocket, [u8; 32]>,
        ping_payload: [u8; 32],
        started: Instant,
    },
    Shutdown {
        inner: tokio_io::io::Shutdown<TSocket>,
        ping_time: Duration,
    },
}

impl<TSocket> Future for PingDialer<TSocket>
where TSocket: AsyncRead + AsyncWrite,
{
    type Item = Duration;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let new_state = match self.inner {
                PingDialerInner::Write { ref mut inner } => {
                    let (socket, ping_payload) = try_ready!(inner.poll());
                    PingDialerInner::Flush {
                        inner: tokio_io::io::flush(socket),
                        ping_payload,
                    }
                },
                PingDialerInner::Flush { ref mut inner, ping_payload } => {
                    let socket = try_ready!(inner.poll());
                    let started = Instant::now();
                    PingDialerInner::Read {
                        inner: tokio_io::io::read_exact(socket, [0; 32]),
                        ping_payload,
                        started,
                    }
                },
                PingDialerInner::Read { ref mut inner, ping_payload, started } => {
                    let (socket, obtained) = try_ready!(inner.poll());
                    let ping_time = started.elapsed();
                    if obtained != ping_payload {
                        return Err(io::Error::new(io::ErrorKind::InvalidData,
                            "Received ping payload doesn't match expected"));
                    }
                    PingDialerInner::Shutdown {
                        inner: tokio_io::io::shutdown(socket),
                        ping_time,
                    }
                },
                PingDialerInner::Shutdown { ref mut inner, ping_time } => {
                    let _ = try_ready!(inner.poll());
                    return Ok(Async::Ready(ping_time));
                },
            };

            self.inner = new_state;
        }
    }
}

/// Enum to merge the output of `Ping` for the dialer and listener.
#[derive(Debug, Copy, Clone)]
pub enum PingOutput {
    /// Received a ping and sent back a pong.
    Pong,
    /// Sent a ping and received back a pong. Contains the ping time.
    Ping(Duration),
}

impl From<Duration> for PingOutput {
    #[inline]
    fn from(duration: Duration) -> PingOutput {
        PingOutput::Ping(duration)
    }
}

impl From<()> for PingOutput {
    #[inline]
    fn from(_: ()) -> PingOutput {
        PingOutput::Pong
    }
}

#[cfg(test)]
mod tests {
    use tokio_tcp::{TcpListener, TcpStream};
    use super::Ping;
    use futures::{Future, Stream};
    use libp2p_core::upgrade;

    // TODO: rewrite tests with the MemoryTransport

    #[test]
    fn ping_pong() {
        let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let listener_addr = listener.local_addr().unwrap();

        let server = listener
            .incoming()
            .into_future()
            .map_err(|(e, _)| e)
            .and_then(|(c, _)| {
                upgrade::apply_inbound(c.unwrap(), Ping::default()).map_err(|_| panic!())
            });

        let client = TcpStream::connect(&listener_addr)
            .and_then(|c| {
                upgrade::apply_outbound(c, Ping::default()).map_err(|_| panic!())
            })
            .map(|_| ());

        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(server.select(client).map_err(|_| panic!())).unwrap();
    }
}
