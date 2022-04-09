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

use futures::prelude::*;
use instant::Instant;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_swarm::NegotiatedSubstream;
use rand::{distributions, prelude::*};
use std::{io, iter, time::Duration};
use void::Void;

/// The `Ping` protocol upgrade.
///
/// The ping protocol sends 32 bytes of random data in configurable
/// intervals over a single outbound substream, expecting to receive
/// the same bytes as a response. At the same time, incoming pings
/// on inbound substreams are answered by sending back the received bytes.
///
/// At most a single inbound and outbound substream is kept open at
/// any time. In case of a ping timeout or another error on a substream, the
/// substream is dropped. If a configurable number of consecutive
/// outbound pings fail, the connection is closed.
///
/// Successful pings report the round-trip time.
///
/// > **Note**: The round-trip time of a ping may be subject to delays induced
/// >           by the underlying transport, e.g. in the case of TCP there is
/// >           Nagle's algorithm, delayed acks and similar configuration options
/// >           which can affect latencies especially on otherwise low-volume
/// >           connections.
#[derive(Default, Debug, Copy, Clone)]
pub struct Ping;

const PING_SIZE: usize = 32;

impl UpgradeInfo for Ping {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/ipfs/ping/1.0.0")
    }
}

impl InboundUpgrade<NegotiatedSubstream> for Ping {
    type Output = NegotiatedSubstream;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, stream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        future::ok(stream)
    }
}

impl OutboundUpgrade<NegotiatedSubstream> for Ping {
    type Output = NegotiatedSubstream;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, stream: NegotiatedSubstream, _: Self::Info) -> Self::Future {
        future::ok(stream)
    }
}

/// Sends a ping and waits for the pong.
pub async fn send_ping<S>(mut stream: S) -> io::Result<(S, Duration)>
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
pub async fn recv_ping<S>(mut stream: S) -> io::Result<S>
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
    use super::*;
    use libp2p_core::{
        multiaddr::multiaddr,
        transport::{memory::MemoryTransport, ListenerEvent, Transport},
    };
    use rand::{thread_rng, Rng};
    use std::time::Duration;

    #[test]
    fn ping_pong() {
        let mem_addr = multiaddr![Memory(thread_rng().gen::<u64>())];
        let mut listener = MemoryTransport.listen_on(mem_addr).unwrap();

        let listener_addr =
            if let Some(Some(Ok(ListenerEvent::NewAddress(a)))) = listener.next().now_or_never() {
                a
            } else {
                panic!("MemoryTransport not listening on an address!");
            };

        async_std::task::spawn(async move {
            let listener_event = listener.next().await.unwrap();
            let (listener_upgrade, _) = listener_event.unwrap().into_upgrade().unwrap();
            let conn = listener_upgrade.await.unwrap();
            recv_ping(conn).await.unwrap();
        });

        async_std::task::block_on(async move {
            let c = MemoryTransport.dial(listener_addr).unwrap().await.unwrap();
            let (_, rtt) = send_ping(c).await.unwrap();
            assert!(rtt > Duration::from_secs(0));
        });
    }
}
