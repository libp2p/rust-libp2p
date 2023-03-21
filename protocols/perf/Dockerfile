# syntax=docker/dockerfile:1.5-labs
FROM rust:1.67.0 as builder

# Run with access to the target cache to speed up builds
WORKDIR /workspace
ADD . .
RUN --mount=type=cache,target=./target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --package libp2p-perf

RUN --mount=type=cache,target=./target \
    mv ./target/release/perf-server /usr/local/bin/perf-server

RUN --mount=type=cache,target=./target \
    mv ./target/release/perf-client /usr/local/bin/perf-client

FROM debian:bullseye-slim

COPY --from=builder /usr/local/bin/perf-server /usr/local/bin/perf-server
COPY --from=builder /usr/local/bin/perf-client /usr/local/bin/perf-client

ENTRYPOINT [ "perf-server"]
