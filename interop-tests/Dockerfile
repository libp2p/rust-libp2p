# syntax=docker/dockerfile:1.5-labs
FROM rust:1.67.0 as builder

# Install zlib as long as libp2p-websocket requires it: https://github.com/paritytech/soketto/issues/72
RUN apt-get update && \
    apt-get download zlib1g && \
    mkdir /dpkg && \
    for deb in *.deb; do dpkg --extract $deb /dpkg || exit 10; done

# Run with access to the target cache to speed up builds
WORKDIR /workspace
ADD . .
RUN --mount=type=cache,target=./target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --package interop-tests

RUN --mount=type=cache,target=./target \
    mv ./target/release/ping /usr/local/bin/testplan

FROM gcr.io/distroless/cc
COPY --from=builder /usr/local/bin/testplan /usr/local/bin/testplan
COPY --from=builder /dpkg /
ENV RUST_BACKTRACE=1
ENTRYPOINT ["testplan"]
