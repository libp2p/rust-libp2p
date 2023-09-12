# syntax=docker/dockerfile:1.5-labs
FROM rust:1.72.0 as builder

RUN rustup target add x86_64-unknown-linux-musl
RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get install -y musl-dev musl-tools

# Run with access to the target cache to speed up builds
WORKDIR /workspace
ADD . .
RUN --mount=type=cache,target=./target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --package hole-punching-tests --target x86_64-unknown-linux-musl

RUN --mount=type=cache,target=./target \
    mv ./target/x86_64-unknown-linux-musl/release/hp_client /usr/local/bin/hp_client
RUN --mount=type=cache,target=./target \
    mv ./target/x86_64-unknown-linux-musl/release/relay /usr/local/bin/relay

FROM ubuntu:jammy

ARG DEBIAN_FRONTEND=noninteractive
RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get install -y iproute2
# Debugging tools
RUN --mount=type=cache,target=/var/cache/apt apt-get install -y tcpdump ncat iputils-ping

COPY --from=builder /usr/local/bin/hp_client /usr/bin/hp_client
COPY --from=builder /usr/local/bin/relay /usr/bin/relay

ENV RUST_BACKTRACE=1
