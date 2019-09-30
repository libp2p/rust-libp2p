#!/usr/bin/env bash
# exit immediately when a command fails
set -e
# only exit with zero if all commands of the pipeline exit successfully
set -o pipefail
# error on unset variables
set -u
# print each command before executing it
set -x

SOURCE_PROTO_FILE=$1
DEST_RUST_FILE=$2

sudo docker build -t rust-libp2p-protobuf-builder $(dirname "$0")

sudo docker run --rm \
     -v `pwd`:/usr/code:z \
     -u="$(id -u):$(id -g)" \
     -w /usr/code \
     rust-libp2p-protobuf-builder \
     /bin/bash -c " \
    protoc --rust_out $DEST_RUST_FILE $SOURCE_PROTO_FILE"
