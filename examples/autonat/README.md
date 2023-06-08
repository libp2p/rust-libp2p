## Description

This example consists of a client and a server, which demonstrate the usage of the AutoNAT and identify protocols in **libp2p**.

## Usage

### Client

The client-side part of the example showcases the combination of the AutoNAT and identify protocols.
The identify protocol allows the local peer to determine its external addresses, which are then included in AutoNAT dial-back requests sent to the server.

To run the client example, follow these steps:

1. Start the server by following the instructions provided in the `examples/server` directory.

2. Open a new terminal.

3. Run the following command in the terminal:
   ```sh
   cargo run --bin autonat_client -- --server-address <server-addr> --server-peer-id <server-peer-id> --listen-port <port>
   ```
   Note: The `--listen-port` parameter is optional and allows you to specify a fixed port at which the local client should listen.

### Server

The server-side example demonstrates a basic AutoNAT server that supports the autonat and identify protocols.

To start the server, follow these steps:

1. Open a terminal.

2. Run the following command:
   ```sh
   cargo run --bin autonat_server -- --listen-port <port>
   ```
   Note: The `--listen-port` parameter is optional and allows you to set a fixed port at which the local peer should listen.

## Conclusion

By combining the AutoNAT and identify protocols, the example showcases the establishment of direct connections between peers and the exchange of external address information.
Users can explore the provided client and server code to gain insights into the implementation details and functionality of **libp2p**.
