# Rust-libp2p Browser-Server WebRTC Example

This example demonstrates how to use the `libp2p-webrtc-websys` transport library in a browser to ping the WebRTC Server.
It uses [wasm-pack](https://rustwasm.github.io/docs/wasm-pack/) to build the project for use in the browser.
The resulting `.js` bindings and `.wasm` can be served by any static web server, such as [`http-server`](https://github.com/http-party/http-server).

## Running the example

### WebRTC Server

First, start the [`/server`](./server) in one terminal:

```sh
cd server
cargo run
```

### WebRTC Client

Then, start the client [`/client`](./client) in a separate terminal:

```sh
cd client
wasm-pack build --target web
```

### Serve webpage

Then serve the `index.html` with a static web server, such as [`http-server`](https://github.com/http-party/http-server):

Using [npx](https://docs.npmjs.com/cli/v9/commands/npx) to run `http-server` without installing it globally, in another terminal, run:

```bash
npx http-server
```

or having done `npm install --global http-server`, simply:

```bash
http-server
```

Open a Chrome browser (`libp2p-webrtc-websys` has not yet been tested in Firefox or other browsers) where you will see the browser pinging the server. Open the server console logs to see the server pinging the browser.
