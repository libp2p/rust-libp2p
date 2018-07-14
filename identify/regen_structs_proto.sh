#!/bin/sh

# This script regenerates the `src/structs_proto.rs` file from `structs.proto`.

sudo docker run --rm -v `pwd`:/usr/code:z -w /usr/code rust /bin/bash -c " \
    apt-get update; \
    apt-get install -y protobuf-compiler; \
    cargo install --version 2.0.2 protobuf-codegen; \
    protoc --rust_out . structs.proto"

sudo chown $USER:$USER *.rs

mv -f structs.rs ./src/structs_proto.rs
