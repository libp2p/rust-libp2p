# Interop tests implementation

This folder defines the implementation for the interop tests.

# Running this test locally

You can run this test locally by having a local Redis instance and by having
another peer that this test can dial or listen for. For example to test that we
can dial/listen for ourselves we can do the following:

1. Start redis (needed by the tests): `docker run --rm -p 6379:6379 redis:7-alpine`
2. In one terminal run the dialer: `RUST_LOG=debug redis_addr=localhost:6379 ip="0.0.0.0" transport=tcp security=noise muxer=yamux is_dialer="true" cargo run --bin native_ping`
3. In another terminal, run the listener: `RUST_LOG=debug redis_addr=localhost:6379 ip="0.0.0.0" transport=tcp security=noise muxer=yamux is_dialer="false" cargo run --bin native_ping`

If testing `transport=quic-v1`, then remove `security` and `muxer` variables from command line, because QUIC protocol comes with its own encryption and multiplexing.

To test the interop with other versions do something similar, except replace one
of these nodes with the other version's interop test.

# Running this test with webtransport dialer in browser

To run the webtransport test from within the browser, you'll need the
`chromedriver` in your `$PATH`, compatible with your Chrome browser.
Firefox is not yet supported as it doesn't support all required features yet
(in v114 there is no support for certhashes).

1. Build the wasm package: `wasm-pack build --target web`
2. Run the dialer: `redis_addr=127.0.0.1:6379 ip=0.0.0.0 transport=webtransport is_dialer=true cargo run --bin wasm_ping`

# Running this test with webrtc-direct

To run the webrtc-direct test, you'll need the `chromedriver` in your `$PATH`, compatible with your Chrome browser.

1. Start redis: `docker run --rm -p 6379:6379 redis:7-alpine`.
2. Build the wasm package: `wasm-pack build --target web`
3. With the webrtc-direct listener `RUST_LOG=debug,webrtc=off,webrtc_sctp=off redis_addr="127.0.0.1:6379" ip="0.0.0.0" transport=webrtc-direct is_dialer="false" cargo run --bin native_ping`
4. Run the webrtc-direct dialer: `RUST_LOG=debug,hyper=off redis_addr="127.0.0.1:6379" ip="0.0.0.0" transport=webrtc-direct is_dialer=true cargo run --bin wasm_ping`

# Running all interop tests locally with Compose

To run this test against all released libp2p versions you'll need to have the
(libp2p/test-plans)[https://github.com/libp2p/test-plans] checked out. Then do
the following (from the root directory of this repository):

1. Build the image: `docker build -t rust-libp2p-head . -f interop-tests/Dockerfile`.
2. Build the images for all released versions in `libp2p/test-plans`: `(cd <path to >/libp2p/test-plans/multidim-interop/ && make)`.
3. Run the test:
```
RUST_LIBP2P="$PWD"; (cd <path to >/libp2p/test-plans/multidim-interop/ && npm run test -- --extra-version=$RUST_LIBP2P/interop-tests/ping-version.json --name-filter="rust-libp2p-head")
```
