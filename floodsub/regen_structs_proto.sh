#!/bin/sh

# This script regenerates the `src/rpc_proto.rs` file from `rpc.proto`.

docker run --rm -v `pwd`:/usr/code:z -w /usr/code rust /bin/bash -c " \
    apt-get update; \
    apt-get install -y protobuf-compiler; \
    cargo install protobuf; \
    protoc --rust_out . rpc.proto"

mv -f rpc.rs ./src/rpc_proto.rs
