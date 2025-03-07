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

use std::{io, time::Duration};

use futures::prelude::*;
use libp2p_swarm::StreamProtocol;
use rand::{distributions, prelude::*};
use web_time::Instant;

pub const PROTOCOL_NAME: StreamProtocol = StreamProtocol::new("/ipfs/ping/1.0.0");

/// The `Ping` protocol upgrade.
///
/// The ping protocol sends 32 bytes of random data in configurable
/// intervals over a single outbound substream, expecting to receive
/// the same bytes as a response. At the same time, incoming pings
/// on inbound substreams are answered by sending back the received bytes.
///
/// At most a single inbound and outbound substream is kept open at
/// any time. In case of a ping timeout or another error on a substream, the
/// substream is dropped.
///
/// Successful pings report the round-trip time.
///
/// > **Note**: The round-trip time of a ping may be subject to delays induced
/// > by the underlying transport, e.g. in the case of TCP there is
/// > Nagle's algorithm, delayed acks and similar configuration options
/// > which can affect latencies especially on otherwise low-volume
/// > connections.
const PING_SIZE: usize = 32;

/// Sends a ping and waits for the pong.
pub(crate) async fn send_ping<S>(mut stream: S) -> io::Result<(S, Duration)>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let payload: [u8; PING_SIZE] = thread_rng().sample(distributions::Standard);
    stream.write_all(&payload).await?;
    stream.flush().await?;
    let started = Instant::now();
    let mut recv_payload = [0u8; PING_SIZE];
    stream.read_exact(&mut recv_payload).await?;
    if recv_payload == payload {
        Ok((stream, started.elapsed()))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Ping payload mismatch",
        ))
    }
}

/// Waits for a ping and sends a pong.
pub(crate) async fn recv_ping<S>(mut stream: S) -> io::Result<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut payload = [0u8; PING_SIZE];
    stream.read_exact(&mut payload).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use libp2p_core::{
        multiaddr::multiaddr,
        transport::{memory::MemoryTransport, DialOpts, ListenerId, PortUse, Transport},
        Endpoint,
    };

    use super::*;

    #[tokio::test]
    async fn ping_pong() {
        let mem_addr = multiaddr![Memory(thread_rng().gen::<u64>())];
        let mut transport = MemoryTransport::new().boxed();
        transport.listen_on(ListenerId::next(), mem_addr).unwrap();

        let listener_addr = transport
            .select_next_some()
            .now_or_never()
            .and_then(|ev| ev.into_new_address())
            .expect("MemoryTransport not listening on an address!");

        tokio::spawn(async move {
            let transport_event = transport.next().await.unwrap();
            let (listener_upgrade, _) = transport_event.into_incoming().unwrap();
            let conn = listener_upgrade.await.unwrap();
            recv_ping(conn).await.unwrap();
        });

        let c = MemoryTransport::new()
            .dial(
                listener_addr,
                DialOpts {
                    role: Endpoint::Dialer,
                    port_use: PortUse::Reuse,
                },
            )
            .unwrap()
            .await
            .unwrap();
        let (_, rtt) = send_ping(c).await.unwrap();
        assert!(rtt > Duration::from_secs(0));
    }
}
