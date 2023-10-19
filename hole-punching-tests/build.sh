#!/bin/bash

# Translate Docker platform format to Rust target format
case "$(echo $1 | cut -d/ -f2)" in
    "amd64") RUST_TARGET="x86_64-unknown-linux-musl";;
    "arm64") RUST_TARGET="aarch64-unknown-linux-musl";;
    *) echo "Unsupported architecture: $1" >&2; exit 1;;
esac

# Add the Rust target
rustup target add ${RUST_TARGET}

apt-get update && apt-get install -y musl-dev musl-tools

# Build the project
cargo build --release --package hole-punching-tests --target ${RUST_TARGET}

# Move the built binary to a common location
mv ./target/${RUST_TARGET}/release/hole-punching-tests /usr/local/bin/hole-punching-tests
