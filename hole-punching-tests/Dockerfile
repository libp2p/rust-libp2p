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
    mv ./target/x86_64-unknown-linux-musl/release/hole-punching-tests /usr/local/bin/hole-punching-tests

FROM alpine:3

ARG DEBIAN_FRONTEND=noninteractive
RUN --mount=type=cache,target=/var/cache/apk apk add nftables

COPY --from=builder /usr/local/bin/hole-punching-tests /usr/bin/hole-punching-tests

ENV RUST_BACKTRACE=1
