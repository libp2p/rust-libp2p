# syntax=docker/dockerfile:1.5-labs
FROM rust:1.72.0 as builder

ARG RUST_TARGET

RUN rustup target add $RUST_TARGET

RUN --mount=type=cache,target=/var/cache/apt apt-get update && apt-get install -y musl-dev musl-tools

# Run with access to the target cache to speed up builds
WORKDIR /workspace
ADD . .
RUN --mount=type=cache,target=./target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --package hole-punching-tests --target $RUST_TARGET

RUN --mount=type=cache,target=./target \
    mv ./target/$RUST_TARGET/release/hole-punching-tests /usr/local/bin/hole-punching-tests

FROM alpine:3
COPY --from=builder /usr/local/bin/hole-punching-tests /usr/bin/hole-punch-client
RUN --mount=type=cache,target=/var/cache/apk apk add bind-tools jq curl tcpdump
ENV RUST_BACKTRACE=1
