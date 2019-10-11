#!/usr/bin/env bash
# exit immediately when a command fails
set -e
# only exit with zero if all commands of the pipeline exit successfully
set -o pipefail
# error on unset variables
set -u
# print each command before executing it
set -x


# The source .proto file.
SOURCE_PROTO_FILE=$1

DEST_FOLDER=$(dirname "$SOURCE_PROTO_FILE")

# The .rs file generated via protoc.
TMP_GEN_RUST_FILE=${SOURCE_PROTO_FILE/proto/rs}

# The above with `_proto` injected.
FINAL_GEN_RUST_FILE=${TMP_GEN_RUST_FILE/.rs/_proto.rs}


sudo docker build -t rust-libp2p-protobuf-builder $(dirname "$0")

sudo docker run --rm \
     -v `pwd`:/usr/code:z \
     -u="$(id -u):$(id -g)" \
     -w /usr/code \
     rust-libp2p-protobuf-builder \
     /bin/bash -c " \
    protoc --rust_out $DEST_FOLDER $SOURCE_PROTO_FILE"


mv $TMP_GEN_RUST_FILE $FINAL_GEN_RUST_FILE
