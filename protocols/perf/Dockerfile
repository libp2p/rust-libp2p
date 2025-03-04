# syntax=docker/dockerfile:1.5-labs
FROM rust:1.83.0 as builder

# Run with access to the target cache to speed up builds
WORKDIR /workspace
ADD . .
RUN --mount=type=cache,target=./target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --package libp2p-perf

RUN --mount=type=cache,target=./target \
    mv ./target/release/perf /usr/local/bin/perf

FROM debian:bullseye-slim

COPY --from=builder /usr/local/bin/perf /app/perf

ENTRYPOINT [ "/app/perf" ]
