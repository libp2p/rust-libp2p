# Manually run tests

First you need to build and start the echo-server:

```bash
# ie. in wasm-tests/webrtc/server 
cd server
cargo run
```

In another terminal run in this current directory (`wasm-tests/webrtc`):

```bash
# ie. in wasm-tests/webrtc
wasm-pack test --chrome --headless
```
