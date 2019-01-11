// Copyright 2019 Parity Technologies (UK) Ltd.
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

use criterion::{Bencher, Criterion, criterion_main, criterion_group};
use futures::prelude::*;
use libp2p_core::Transport;
use tokio::{
    io,
    runtime::current_thread::Runtime
};

fn secio_and_send_data(bench: &mut Bencher, data: &[u8]) {
    let key = libp2p_secio::SecioKeyPair::ed25519_generated().unwrap();
    let transport =
        libp2p_tcp::TcpConfig::new().with_upgrade(libp2p_secio::SecioConfig::new(key));

    let data_vec = data.to_vec();

    bench.iter(move || {
        let (listener, addr) = transport
            .clone()
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();

        let listener_side = listener
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, _)| client.unwrap().0)
            .map_err(|_| panic!())
            .and_then(|client| io::read_to_end(client.stream, Vec::new()))
            .and_then(|msg| {
                assert_eq!(msg.1, data);
                Ok(())
            });

        let dialer_side = transport
            .clone()
            .dial(addr)
            .unwrap()
            .map_err(|_| panic!())
            .and_then(|server| io::write_all(server.stream, data_vec.clone()))
            .map(|_| ());

        let combined = listener_side.select(dialer_side)
            .map_err(|(err, _)| panic!("{:?}", err))
            .map(|_| ());
        let mut rt = Runtime::new().unwrap();
        rt.block_on(combined).unwrap();
    })
}

fn raw_tcp_connect_and_send_data(bench: &mut Bencher, data: &[u8]) {
    let transport = libp2p_tcp::TcpConfig::new();
    let data_vec = data.to_vec();

    bench.iter(move || {
        let (listener, addr) = transport
            .clone()
            .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
            .unwrap();

        let listener_side = listener
            .into_future()
            .map_err(|(err, _)| err)
            .and_then(|(client, _)| client.unwrap().0)
            .map_err(|_| panic!())
            .and_then(|client| io::read_to_end(client, Vec::new()))
            .and_then(|msg| {
                assert_eq!(msg.1, data);
                Ok(())
            });

        let dialer_side = transport
            .clone()
            .dial(addr)
            .unwrap()
            .map_err(|_| panic!())
            .and_then(|server| io::write_all(server, data_vec.clone()))
            .map(|_| ());

        let combined = listener_side.select(dialer_side)
            .map_err(|(err, _)| panic!("{:?}", err))
            .map(|_| ());
        let mut rt = Runtime::new().unwrap();
        rt.block_on(combined).unwrap();
    })
}

fn criterion_benchmarks(bench: &mut Criterion) {
    bench.bench_function("secio_connect_and_send_hello", move |b| secio_and_send_data(b, b"hello world"));
    bench.bench_function("raw_tcp_connect_and_send_hello", move |b| raw_tcp_connect_and_send_data(b, b"hello world"));

    let data = (0 .. 1024).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
    bench.bench_function("secio_connect_and_send_one_kb", { let data = data.clone(); move |b| secio_and_send_data(b, &data) });
    bench.bench_function("raw_tcp_connect_and_send_one_kb", move |b| raw_tcp_connect_and_send_data(b, &data));

    let data = (0 .. 1024 * 1024).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
    bench.bench_function("secio_connect_and_send_one_mb", { let data = data.clone(); move |b| secio_and_send_data(b, &data) });
    bench.bench_function("raw_tcp_connect_and_send_one_mb", move |b| raw_tcp_connect_and_send_data(b, &data));

    let data = (0 .. 2 * 1024 * 1024).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
    bench.bench_function("secio_connect_and_send_two_mb", { let data = data.clone(); move |b| secio_and_send_data(b, &data) });
    bench.bench_function("raw_tcp_connect_and_send_two_mb", move |b| raw_tcp_connect_and_send_data(b, &data));
}

criterion_group!(benches, criterion_benchmarks);
criterion_main!(benches);
