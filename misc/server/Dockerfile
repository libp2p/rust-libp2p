# syntax=docker/dockerfile:1.5-labs
FROM rust:1.75.0 as chef
RUN wget -q -O- https://github.com/LukeMathWalker/cargo-chef/releases/download/v0.1.62/cargo-chef-x86_64-unknown-linux-gnu.tar.gz | tar -zx -C /usr/local/bin
RUN cargo install --locked --root /usr/local libp2p-lookup --version 0.6.4
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --package libp2p-server --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --package libp2p-server

FROM gcr.io/distroless/cc
COPY --from=builder /usr/local/bin/libp2p-server /usr/local/bin/libp2p-lookup /usr/local/bin/
CMD ["libp2p-server"]
