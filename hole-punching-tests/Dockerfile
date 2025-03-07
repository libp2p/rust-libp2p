# syntax=docker/dockerfile:1.5-labs
FROM rust:1.83.0 as builder

# Run with access to the target cache to speed up builds
WORKDIR /workspace
ADD . .

# Build the relay as a statically-linked binary. Unfortunately, we must specify the `--target` explicitly. See https://msfjarvis.dev/posts/building-static-rust-binaries-for-linux/.
RUN --mount=type=cache,target=./target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    RUSTFLAGS='-C target-feature=+crt-static' cargo build --release --package hole-punching-tests --target $(rustc -vV | grep host | awk '{print $2}')

RUN --mount=type=cache,target=./target \
    mv ./target/$(rustc -vV | grep host | awk '{print $2}')/release/hole-punching-tests /usr/local/bin/hole-punching-tests

FROM alpine:3
COPY --from=builder /usr/local/bin/hole-punching-tests /usr/bin/hole-punch-client
RUN --mount=type=cache,target=/var/cache/apk apk add bind-tools jq curl tcpdump iproute2-tc
ENV RUST_BACKTRACE=1
