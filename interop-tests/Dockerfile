# syntax=docker/dockerfile:1.5-labs
FROM rust:1.67.0

# Setup protoc. TODO this breaks reproducibility and uses an old version of protoc.
# In the future protobuf generated files will be checked in, so we can remove this
WORKDIR /protoc-setup
RUN apt-get update && apt-get install -y cmake

# Run with access to the target cache to speed up builds
WORKDIR /workspace
ADD . .
RUN --mount=type=cache,target=./target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --package interop-tests

RUN --mount=type=cache,target=./target \
    mv ./target/debug/ping /usr/local/bin/testplan

ENV RUST_BACKTRACE=1
ENTRYPOINT ["testplan"]
