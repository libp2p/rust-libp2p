use asynchronous_codec::Encoder;
use bytes::BytesMut;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use quick_protobuf_codec::{proto, Codec};

pub fn benchmark(c: &mut Criterion) {
    for size in [1000, 10_000, 100_000, 1_000_000, 10_000_000] {
        c.bench_with_input(BenchmarkId::new("encode", size), &size, |b, i| {
            b.iter_batched(
                || {
                    let mut out = BytesMut::new();
                    out.reserve(i + 100);
                    let codec = Codec::<proto::Message>::new(i + 100);
                    let msg = proto::Message {
                        data: vec![0; size],
                    };

                    (codec, out, msg)
                },
                |(mut codec, mut out, msg)| codec.encode(msg, &mut out).unwrap(),
                BatchSize::SmallInput,
            );
        });
    }
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
