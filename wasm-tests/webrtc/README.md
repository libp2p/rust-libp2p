# Manually run tests

First you need to build and start the echo-server:

```bash
cd server
cargo run
```

In another terminal run:

```bash
wasm-pack test --chrome --headless
```
