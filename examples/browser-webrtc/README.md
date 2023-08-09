# Rust-Libp2p Browser-Server WebRTC Example

This example demonstrates how to use the `rust-libp2p` library in a browser.
It uses [Leptos](https://leptos.dev/) WebAssembly framework to display the pings from the server.

## Running the example

First, start the `/server`:

```sh
cd server
cargo run
```

Then, start the Leptos `/client`:

```sh
cd client
trunk serve --open
```

This will open the browser where you will see the browser pinging the server. Open the server console logs to see the server pinging the browser.
