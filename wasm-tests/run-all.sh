#!/bin/bash
set -e

# cd to this script directory
cd "$(dirname "${BASH_SOURCE[0]}")" || exit 1

./webtransport-tests/run.sh

# `libp2p-dns` compiles its DNS-over-HTTPS resolver only for `wasm32`, so its
# unit tests are unreachable from the native `cargo test` matrix.
wasm-pack test --chrome --headless ../transports/dns
