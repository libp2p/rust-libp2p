#!/bin/bash

# Prefer podman over docker since it doesn't require root privileges
if command -v podman > /dev/null; then
    docker=podman
else
    docker=docker
fi

# cd to this script directory
cd "$(dirname "${BASH_SOURCE[0]}")" || exit 1

# Print the directory for debugging
echo "Tests: $PWD"

# Build and run echo-server
$docker build -t webtransport-echo-server echo-server || exit 1
id="$($docker run -d --network=host webtransport-echo-server)" || exit 1

# Run tests
wasm-pack test --chrome --headless
exit_code=$?

# Remove echo-server container
$docker rm -f "$id"

# Propagate wasm-pack's exit code
exit $exit_code
