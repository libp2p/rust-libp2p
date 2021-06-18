# 1: Build the exe
FROM rust:1 as builder
LABEL Name=libp2p-relay Version=0.0.1
# 1a: Prepare for static linking
RUN apt-get update && \
    apt-get dist-upgrade -y && \
    apt-get install -y musl-tools && \
    rustup target add x86_64-unknown-linux-musl
# 1b: Download and compile Rust dependencies (and store as a separate Docker layer)
WORKDIR /usr/src/libp2p

COPY . .
RUN cargo build --target x86_64-unknown-linux-musl --example=relay --package=libp2p-relay

# 2: Copy the exe and extra files ("static") to an empty Docker image
FROM debian:buster-slim

COPY --from=builder /usr/src/libp2p/target/x86_64-unknown-linux-musl/debug/examples/relay .
USER 1000