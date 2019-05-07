#!/bin/sh

sudo docker run --rm -v `pwd`:/usr/code:z -w /usr/code rust /bin/bash -c " \
    apt-get update; \
    apt-get install -y protobuf-compiler; \
    cargo install --version 2.3.0 protobuf-codegen; \
    protoc --rust_out ./src/io/handshake/ ./src/io/handshake/payload.proto"

sudo chown $USER:$USER ./src/io/handshake/payload.rs
