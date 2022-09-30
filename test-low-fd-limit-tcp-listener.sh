#!/usr/bin/bash

# This file build and run the `tokio_listen` example in a docker container with a low file descriptor limit.
# You can use the `tokio_dial` example to trigger the error. You might need to run it several times before
# the limit is actually hit.
#
# `cargo run --package libp2p-tcp --example tokio_dial --features tokio -- /ip4/127.0.0.1/tcp/9999`

FILE_DESC_LIMIT=12

stop_container() {
  docker rm -f "$LISTEN_CONTAINER";
}

trap stop_container SIGINT;

LISTEN_CONTAINER=$(docker run -d -p 9999:9999 --ulimit nofile="$FILE_DESC_LIMIT":50 "$(docker build . --quiet)")

docker logs -f "$LISTEN_CONTAINER";


