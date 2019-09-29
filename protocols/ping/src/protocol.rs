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
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, upgrade::Negotiated};
use log::debug;
use rand::{distributions, prelude::*};
use std::{io, iter, time::Duration};
use tokio_io::{io as nio, AsyncRead, AsyncWrite};
use wasm_timer::Instant;

/// Represents a prototype for an upgrade to handle the ping protocol.
///
/// The protocol works the following way:
///
/// - Dialer sends 32 bytes of random data.
/// - Listener receives the data and sends it back.
/// - Dialer receives the data and verifies that it matches what it sent.
///
/// The dialer produces a `Duration`, which corresponds to the round-trip time
/// of the payload.
///
/// > **Note**: The round-trip time of a ping may be subject to delays induced
/// >           by the underlying transport, e.g. in the case of TCP there is
/// >           Nagle's algorithm, delayed acks and similar configuration options
/// >           which can affect latencies especially on otherwise low-volume
/// >           connections.
#[derive(Default, Debug, Copy, Clone)]
pub struct Ping;

impl UpgradeInfo for Ping {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/ipfs/ping/1.0.0")
    }
}

type RecvPing<T> = nio::ReadExact<Negotiated<T>, [u8; 32]>;
type SendPong<T> = nio::WriteAll<Negotiated<T>, [u8; 32]>;
type Flush<T> = nio::Flush<Negotiated<T>>;
type Shutdown<T> = nio::Shutdown<Negotiated<T>>;

impl<TSocket> InboundUpgrade<TSocket> for Ping
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = ();
    type Error = io::Error;
    type Future = future::Map<
        future::AndThen<
        future::AndThen<
        future::AndThen<
            RecvPing<TSocket>,
            SendPong<TSocket>, fn((Negotiated<TSocket>, [u8; 32])) -> SendPong<TSocket>>,
            Flush<TSocket>, fn((Negotiated<TSocket>, [u8; 32])) -> Flush<TSocket>>,
            Shutdown<TSocket>, fn(Negotiated<TSocket>) -> Shutdown<TSocket>>,
    fn(Negotiated<TSocket>) -> ()>;

    #[inline]
    fn upgrade_inbound(self, socket: Negotiated<TSocket>, _: Self::Info) -> Self::Future {
        nio::read_exact(socket, [0; 32])
            .and_then::<fn(_) -> _, _>(|(sock, buf)| nio::write_all(sock, buf))
            .and_then::<fn(_) -> _, _>(|(sock, _)| nio::flush(sock))
            .and_then::<fn(_) -> _, _>(|sock| nio::shutdown(sock))
            .map(|_| ())
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for Ping
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = Duration;
    type Error = io::Error;
    type Future = PingDialer<Negotiated<TSocket>>;

    #[inline]
    fn upgrade_outbound(self, socket: Negotiated<TSocket>, _: Self::Info) -> Self::Future {
        let payload: [u8; 32] = thread_rng().sample(distributions::Standard);
        debug!("Preparing ping payload {:?}", payload);

        PingDialer {
            state: PingDialerState::Write {
                inner: nio::write_all(socket, payload),
            },
        }
    }
}

/// A `PingDialer` is a future that sends a ping and expects to receive a pong.
pub struct PingDialer<TSocket> {
    state: PingDialerState<TSocket>
}

enum PingDialerState<TSocket> {
    Write {
        inner: nio::WriteAll<TSocket, [u8; 32]>,
    },
    Flush {
        inner: nio::Flush<TSocket>,
        payload: [u8; 32],
    },
    Read {
        inner: nio::ReadExact<TSocket, [u8; 32]>,
        payload: [u8; 32],
        started: Instant,
    },
    Shutdown {
        inner: nio::Shutdown<TSocket>,
        rtt: Duration,
    },
}

impl<TSocket> Future for PingDialer<TSocket>
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Item = Duration;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            self.state = match self.state {
                PingDialerState::Write { ref mut inner } => {
                    let (socket, payload) = try_ready!(inner.poll());
                    PingDialerState::Flush {
                        inner: nio::flush(socket),
                        payload,
                    }
                },
                PingDialerState::Flush { ref mut inner, payload } => {
                    let socket = try_ready!(inner.poll());
                    let started = Instant::now();
                    PingDialerState::Read {
                        inner: nio::read_exact(socket, [0; 32]),
                        payload,
                        started,
                    }
                },
                PingDialerState::Read { ref mut inner, payload, started } => {
                    let (socket, payload_received) = try_ready!(inner.poll());
                    let rtt = started.elapsed();
                    if payload_received != payload {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData, "Ping payload mismatch"));
                    }
                    PingDialerState::Shutdown {
                        inner: nio::shutdown(socket),
                        rtt,
                    }
                },
                PingDialerState::Shutdown { ref mut inner, rtt } => {
                    try_ready!(inner.poll());
                    return Ok(Async::Ready(rtt));
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Ping;
    use futures::prelude::*;
    use libp2p_core::{
        upgrade,
        multiaddr::multiaddr,
        transport::{
            Transport,
            ListenerEvent,
            memory::MemoryTransport
        }
    };
    use rand::{thread_rng, Rng};
    use std::time::Duration;

    #[test]
    fn ping_pong() {
        let mem_addr = multiaddr![Memory(thread_rng().gen::<u64>())];
        let mut listener = MemoryTransport.listen_on(mem_addr).unwrap();

        let listener_addr =
            if let Ok(Async::Ready(Some(ListenerEvent::NewAddress(a)))) = listener.poll() {
                a
            } else {
                panic!("MemoryTransport not listening on an address!");
            };

        let server = listener
            .into_future()
            .map_err(|(e, _)| e)
            .and_then(|(listener_event, _)| {
                let (listener_upgrade, _) = listener_event.unwrap().into_upgrade().unwrap();
                let conn = listener_upgrade.wait().unwrap();
                upgrade::apply_inbound(conn, Ping::default())
                    .map_err(|e| panic!(e))
            });

        let client = MemoryTransport.dial(listener_addr).unwrap()
            .and_then(|c| {
                upgrade::apply_outbound(c, Ping::default(), upgrade::Version::V1)
                    .map_err(|e| panic!(e))
            });

        let mut runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.spawn(server.map_err(|e| panic!(e)));
        let rtt = runtime.block_on(client).expect("RTT");
        assert!(rtt > Duration::from_secs(0));
    }
}
