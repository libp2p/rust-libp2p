#!/bin/bash

LOCAL_CACHE_ROOT="$HOME/.wasm-tests-cache";  # must be absolute path

if [[ ! -d $LOCAL_CACHE_ROOT ]]; then
    mkdir -vp \
        "$LOCAL_CACHE_ROOT/cargo/registry" \
        "$LOCAL_CACHE_ROOT/cargo/git";
    sudo chown -R 1001:1001 "$LOCAL_CACHE_ROOT";
fi

SCRIPT_PATH=$(realpath $(dirname "${BASH_SOURCE[0]}"))
cd "$SCRIPT_PATH" || exit 1

echo "Tests: $PWD"

docker build -f Dockerfile.wasm-builder -t websocket-tests . || exit 1

docker run --rm -ti --network=host --user 1001 --privileged -v "$SCRIPT_PATH/../..:/app" websocket-tests
