# syntax=docker/dockerfile:1.5-labs
FROM rust:1.83.0 as chef
RUN wget -q -O- https://github.com/LukeMathWalker/cargo-chef/releases/download/v0.1.62/cargo-chef-x86_64-unknown-linux-gnu.tar.gz | tar -zx -C /usr/local/bin
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
CMD ["libp2p-server"]
