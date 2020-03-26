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
//! This benchmark tests the speed of cloning of the largest possible inlined
//! multiaddr vs the smalles possible heap allocated multiaddr.
//!
//! Note that the main point of the storage optimization is not to speed up clone
//! but to avoid allocating on the heap at all, but still you see a nice benefit
//! in the speed of cloning.
use criterion::{Bencher, Criterion, criterion_main, criterion_group, black_box};
use parity_multiaddr::Multiaddr;

fn do_clone(multiaddr: &Multiaddr) -> usize {
    let mut res = 0usize;
    for _ in 0..10 {
        res += multiaddr.clone().as_ref().len()
    }
    res
}

fn clone(bench: &mut Bencher, addr: &Multiaddr) {
    bench.iter(|| do_clone(black_box(addr)))
}

fn criterion_benchmarks(bench: &mut Criterion) {
    let inlined: Multiaddr = "/dns4/01234567890123456789123/tcp/80/ws".parse().unwrap();
    let heap: Multiaddr = "/dns4/0123456789012345678901234/tcp/80/ws".parse().unwrap();
    assert_eq!(inlined.as_ref().len(), 30);
    assert_eq!(heap.as_ref().len(), 32);

    bench.bench_function("clone 10 max inlined", |b| clone(b, &inlined));
    bench.bench_function("clone 10 min heap", |b| clone(b, &heap));
}

criterion_group!(benches, criterion_benchmarks);
criterion_main!(benches);
