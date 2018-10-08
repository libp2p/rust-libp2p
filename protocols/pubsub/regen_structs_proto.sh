#!/bin/sh

# This script regenerates the `src/rpc_proto.rs` file from `rpc.proto`.

docker run --rm -v `pwd`:/usr/code:z -w /usr/code rust /bin/bash -c " \
    apt-get update; \
    apt-get install -y protobuf-compiler; \
    cargo install --version 2.0.2 protobuf-codegen; \
    protoc --rust_out . rpc.proto"

sudo chown $USER:$USER *.rs

mv -f rpc.rs ./src/rpc_proto.rs
