#!/bin/bash

# Run tests
wasm-pack test --node
exit_code=$?

# Propagate wasm-pack's exit code
exit $exit_code
