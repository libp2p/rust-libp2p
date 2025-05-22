## Description

The **libp2p** relay example showcases how to create a relay node that can route messages between different peers in a p2p network.

## Usage

To run the example, follow these steps:

#### 1. Run the relay nodes:
##### Running with Cargo

You can start the relay node manually using Cargo and the available command-line arguments:

```sh
cargo run -- --port <port> --secret-key-seed <seed> [--websocket-port <ws-port>] [--webrtc-port <wrtc-port>]
```

- Replace `<port>` with the port number on which the relay node will listen for incoming connections (TCP and QUIC).
- Replace `<seed>` with a seed value used to generate a deterministic peer ID for the relay node.
- Use `--websocket-port <ws-port>` to enable WebSocket support on the specified port.
- Use `--webrtc-port <wrtc-port>` to enable WebRTC support on the specified port.
- If you do **not** provide `--websocket-port` or `--webrtc-port`, the relay will **not** listen on those transports.

**Example:**
```sh
cargo run -- --port 9000 --secret-key-seed 42 --websocket-port 8080 --webrtc-port 8081
```
This will start the relay node with TCP and QUIC on port 9000, WebSocket on port 8080, and WebRTC on port 8081.

##### Using Provided Scripts

To simplify repeated tests and ensure deterministic peer IDs and multiaddresses, you can use the provided `run*.sh` scripts. These scripts automate the process of starting the relay server with predefined seeds and ports, making it easier to connect different clients for testing.

- **Run the relay server with only TCP and QUIC transports:**
  ```sh
  ./run-relay.sh
  ```
  This script starts the relay server with a fixed secret key seed and port, but **only enables TCP and QUIC** transports (no WebSocket or WebRTC).

- **Run the relay server with WebRTC support:**
  ```sh
  ./run-relay-webrtc.sh
  ```
  This script starts the relay server with WebRTC enabled on a specified port.

- **Run the relay server with WebSocket support:**
  ```sh
  ./run-relay-websocket.sh
  ```
  This script starts the relay server with WebSocket enabled on a specified port.

- **Run the relay server with all supported transports (TCP, QUIC, WebRTC, and WebSocket):**
  ```sh
  ./run-relay-all.sh
  ```
  This script starts the relay server with TCP, QUIC, WebSocket, and WebRTC enabled, using deterministic parameters for reproducible tests:
  ```sh
  RUST_LOG=info cargo run -- --port 4884 --secret-key-seed 0 --websocket-port 8080 --webrtc-port 8081
  ```

**Note:**  
If you do not provide a port for `--websocket-port` or `--webrtc-port`, the relay will not listen on those transports. Port `0` is not supported for WebSocket or WebRTC in this example; you must specify a fixed port number.

Check the script files for details on the specific seeds, ports, and options used. You can modify these scripts to match your testing requirements.


#### 2. The relay node will start listening for incoming connections.
   It will print the listening address once it is ready.

#### 3. Connect other **libp2p** nodes to the relay node by specifying the relay's listening address as one of the bootstrap nodes in their configuration.

#### 4. Once the connections are established, the relay node will facilitate communication between the connected peers, allowing them to exchange messages and data.

## Conclusion

The **libp2p** relay example demonstrates how to implement a relay node.
By running a relay node and connecting other **libp2p** nodes to it, users can create a decentralized network where peers can communicate and interact with each other.
