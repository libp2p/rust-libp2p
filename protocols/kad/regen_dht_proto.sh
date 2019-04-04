#!/bin/sh

# This script regenerates the `src/dht_proto.rs` file from `dht.proto`.

docker run --rm -v `pwd`:/usr/code:z -w /usr/code rust /bin/bash -c " \
    apt-get update; \
    apt-get install -y protobuf-compiler; \
    cargo install --version 2.3.0 protobuf-codegen; \
    protoc --rust_out . dht.proto;\
    protoc --rust_out . record.proto"

sudo chown $USER:$USER *.rs

mv -f dht.rs ./src/protobuf_structs/dht.rs
mv -f record.rs ./src/protobuf_structs/record.rs
