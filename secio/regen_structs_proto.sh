#!/bin/sh

# This script regenerates the `src/structs_proto.rs` and `src/keys_proto.rs` files from
# `structs.proto` and `keys.proto`.

sudo docker run --rm -v `pwd`:/usr/code:z -w /usr/code rust /bin/bash -c " \
    apt-get update; \
    apt-get install -y protobuf-compiler; \
    cargo install protobuf; \
    protoc --rust_out . structs.proto; \
    protoc --rust_out . keys.proto"

mv -f structs.rs ./src/structs_proto.rs
mv -f keys.rs ./src/keys_proto.rs
