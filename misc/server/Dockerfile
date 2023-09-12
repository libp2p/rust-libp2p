FROM rust:1.72-bullseye as builder
WORKDIR /usr/src/rust-libp2p-server

# Run with access to the target cache to speed up builds
WORKDIR /workspace

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo install --locked --root /usr/local libp2p-lookup  --version 0.6.4

ADD . .
RUN --mount=type=cache,target=./target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --package libp2p-server

RUN --mount=type=cache,target=./target \
    mv ./target/release/libp2p-server /usr/local/bin/libp2p-server

FROM gcr.io/distroless/cc
COPY --from=builder /usr/local/bin/libp2p-server /usr/local/bin/libp2p-lookup /usr/local/bin/
CMD ["libp2p-server"]
