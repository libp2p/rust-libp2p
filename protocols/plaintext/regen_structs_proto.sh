#!/bin/sh

docker run --rm -v "`pwd`/../../":/usr/code:z -w /usr/code rust /bin/bash -c " \
    apt-get update; \
    apt-get install -y protobuf-compiler; \
    cargo install --version 2.3.0 protobuf-codegen; \
    protoc --rust_out=./protocols/plaintext/src/pb ./protocols/plaintext/structs.proto;"

