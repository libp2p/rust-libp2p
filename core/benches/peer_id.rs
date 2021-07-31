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

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use libp2p_core::{identity, PeerId};

fn from_bytes(c: &mut Criterion) {
    let peer_id_bytes = identity::Keypair::generate_ed25519()
        .public()
        .to_peer_id()
        .to_bytes();

    c.bench_function("from_bytes", |b| {
        b.iter(|| {
            black_box(PeerId::from_bytes(&peer_id_bytes).unwrap());
        })
    });
}

fn clone(c: &mut Criterion) {
    let peer_id = identity::Keypair::generate_ed25519().public().to_peer_id();

    c.bench_function("clone", |b| {
        b.iter(|| {
            black_box(peer_id.clone());
        })
    });
}

fn sort_vec(c: &mut Criterion) {
    let peer_ids: Vec<_> = (0..100)
        .map(|_| identity::Keypair::generate_ed25519().public().to_peer_id())
        .collect();

    c.bench_function("sort_vec", |b| {
        b.iter(|| {
            let mut peer_ids = peer_ids.clone();
            peer_ids.sort_unstable();
            black_box(peer_ids);
        })
    });
}

criterion_group!(peer_id, from_bytes, clone, sort_vec);
criterion_main!(peer_id);
