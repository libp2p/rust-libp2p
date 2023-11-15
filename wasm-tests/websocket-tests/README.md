# Manually run tests

These tests are used to test swarm connectivity via websocket in WASM.

## Dockerfile
Dockerfile.wasm-builder is used as an environment to allow build and execution of WASM bindgen tests with chromium driver.

This docker image provisions a container with mounted volume pointing to this project root in `/app`.
It also creates `$HOME/.wasm-tests-cache` to store cargo cache and requires your user permissions to create this directory.

## Running tests
When running server you need to provide its listen address so that it's accessible inside the docker.

1. Run websocket server (outside of docker container):
```bash
export SERVER_LISTEN_MULTIADDRESS="/ip4/<YOUR_LOCAL_IP_ADDRESS>/tcp/18222/ws"
export RUST_LOG=debug
cargo run -p websocket-server
```

On another terminal run client tests:

1. Run this script which will build and run docker container with terminal attached:
```bash
./wasm-tests/websocket-tests/run-docker.sh
```

2. Now inside docker container terminal run tests:
```bash
export SERVER_LISTEN_MULTIADDRESS="/ip4/<YOUR_LOCAL_IP_ADDRESS>/tcp/18222/ws"
export RUST_LOG=debug

cd wasm-tests/websocket-tests/

# test wasm
wasm-pack test --chrome --headless

# test native tokio
cargo test test_with_tokio -- --nocapture
```

> **Note:** These tests are not yet ready for automated CI/CD integration.
