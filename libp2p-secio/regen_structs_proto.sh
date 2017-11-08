#!/bin/sh

# This script regenerates the `src/structs_proto.rs` file from `structs.proto`.

docker run --rm -v `pwd`:/usr/code:z -w /usr/code rust /bin/bash -c " \
    apt-get update; \
    apt-get install -y protobuf-compiler; \
    cargo install protobuf; \
    protoc --rust_out . structs.proto"

mv -f structs.rs ./src/structs_proto.rs
