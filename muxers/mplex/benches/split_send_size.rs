// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! A benchmark for the `split_send_size` configuration option
//! using different transports.

use async_std::task;
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use futures::future::poll_fn;
use futures::prelude::*;
use futures::{channel::oneshot, future::join};
use libp2p::core::muxing::StreamMuxerExt;
use libp2p::core::{
    identity, multiaddr::multiaddr, muxing, transport, upgrade, Multiaddr, PeerId, Transport,
};
use libp2p::mplex;
use libp2p::plaintext::PlainText2Config;
use libp2p::tcp::GenTcpConfig;
use std::pin::Pin;
use std::time::Duration;

type BenchTransport = transport::Boxed<(PeerId, muxing::StreamMuxerBox)>;

/// The sizes (in bytes) used for the `split_send_size` configuration.
const BENCH_SIZES: [usize; 8] = [
    256,
    512,
    1024,
    8 * 1024,
    16 * 1024,
    64 * 1024,
    256 * 1024,
    1024 * 1024,
];

fn prepare(c: &mut Criterion) {
    let _ = env_logger::try_init();

    let payload: Vec<u8> = vec![1; 1024 * 1024];

    let mut tcp = c.benchmark_group("tcp");
    let tcp_addr = multiaddr![Ip4(std::net::Ipv4Addr::new(127, 0, 0, 1)), Tcp(0u16)];
    for &size in BENCH_SIZES.iter() {
        tcp.throughput(Throughput::Bytes(payload.len() as u64));
        let mut receiver_transport = tcp_transport(size);
        let mut sender_transport = tcp_transport(size);
        tcp.bench_function(format!("{}", size), |b| {
            b.iter(|| {
                run(
                    black_box(&mut receiver_transport),
                    black_box(&mut sender_transport),
                    black_box(&payload),
                    black_box(&tcp_addr),
                )
            })
        });
    }
    tcp.finish();

    let mut mem = c.benchmark_group("memory");
    let mem_addr = multiaddr![Memory(0u64)];
    for &size in BENCH_SIZES.iter() {
        mem.throughput(Throughput::Bytes(payload.len() as u64));
        let mut receiver_transport = mem_transport(size);
        let mut sender_transport = mem_transport(size);
        mem.bench_function(format!("{}", size), |b| {
            b.iter(|| {
                run(
                    black_box(&mut receiver_transport),
                    black_box(&mut sender_transport),
                    black_box(&payload),
                    black_box(&mem_addr),
                )
            })
        });
    }
    mem.finish();
}

/// Transfers the given payload between two nodes using the given transport.
fn run(
    receiver_trans: &mut BenchTransport,
    sender_trans: &mut BenchTransport,
    payload: &Vec<u8>,
    listen_addr: &Multiaddr,
) {
    receiver_trans.listen_on(listen_addr.clone()).unwrap();
    let (addr_sender, addr_receiver) = oneshot::channel();
    let mut addr_sender = Some(addr_sender);
    let payload_len = payload.len();

    let receiver = async move {
        loop {
            match receiver_trans.next().await.unwrap() {
                transport::TransportEvent::NewAddress { listen_addr, .. } => {
                    addr_sender.take().unwrap().send(listen_addr).unwrap();
                }
                transport::TransportEvent::Incoming { upgrade, .. } => {
                    let (_peer, mut conn) = upgrade.await.unwrap();
                    let mut s = conn.next_inbound().await.expect("unexpected error");

                    let mut buf = vec![0u8; payload_len];
                    let mut off = 0;
                    loop {
                        // Read in typical chunk sizes of up to 8KiB.
                        let end = off + std::cmp::min(buf.len() - off, 8 * 1024);
                        let n = poll_fn(|cx| Pin::new(&mut s).poll_read(cx, &mut buf[off..end]))
                            .await
                            .unwrap();
                        off += n;
                        if off == buf.len() {
                            return;
                        }
                    }
                }
                _ => panic!("Unexpected transport event"),
            }
        }
    };

    // Spawn and block on the sender, i.e. until all data is sent.
    let sender = async move {
        let addr = addr_receiver.await.unwrap();
        let (_peer, mut conn) = sender_trans.dial(addr).unwrap().await.unwrap();
        let mut stream = conn.next_outbound().await.unwrap();
        let mut off = 0;
        loop {
            let n = poll_fn(|cx| Pin::new(&mut stream).poll_write(cx, &payload[off..]))
                .await
                .unwrap();
            off += n;
            if off == payload.len() {
                poll_fn(|cx| Pin::new(&mut stream).poll_flush(cx))
                    .await
                    .unwrap();
                return;
            }
        }
    };

    // Wait for all data to be received.
    task::block_on(join(sender, receiver));
}

fn tcp_transport(split_send_size: usize) -> BenchTransport {
    let key = identity::Keypair::generate_ed25519();
    let local_public_key = key.public();

    let mut mplex = mplex::MplexConfig::default();
    mplex.set_split_send_size(split_send_size);

    libp2p::tcp::TcpTransport::new(GenTcpConfig::default().nodelay(true))
        .upgrade(upgrade::Version::V1)
        .authenticate(PlainText2Config { local_public_key })
        .multiplex(mplex)
        .timeout(Duration::from_secs(5))
        .boxed()
}

fn mem_transport(split_send_size: usize) -> BenchTransport {
    let key = identity::Keypair::generate_ed25519();
    let local_public_key = key.public();

    let mut mplex = mplex::MplexConfig::default();
    mplex.set_split_send_size(split_send_size);

    transport::MemoryTransport::default()
        .upgrade(upgrade::Version::V1)
        .authenticate(PlainText2Config { local_public_key })
        .multiplex(mplex)
        .timeout(Duration::from_secs(5))
        .boxed()
}

criterion_group!(split_send_size, prepare);
criterion_main!(split_send_size);
