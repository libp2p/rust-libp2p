#!/bin/sh

# Similar to regen_structs_proto.sh but leaves installation of any
# dependencies up to the user.
# TODO: write a script that will install dependencies across platforms.
# Basic tip: run `chmod +x gen_rpc_proto.sh && ./gen_rpc_proto.sh`

protoc --rust_out . rpc.proto
sudo chown $USER:$USER *.rs
mv -f rpc.rs ./src/rpc_proto.rs
