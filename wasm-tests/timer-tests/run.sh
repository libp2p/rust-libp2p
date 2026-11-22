#!/bin/bash

# cd to this script directory
cd "$(dirname "${BASH_SOURCE[0]}")" || exit 1

# Print the directory for debugging
echo "Tests: $PWD"

wasm-pack test --chrome --headless
