## Description

The **libp2p** relay example showcases how to create a relay node that can route messages between different peers in a p2p network.

## Usage

To run the example, follow these steps:

1. Run the relay node by executing the following command:

   ```sh
   cargo run -- --port <port> --secret-key-seed <seed>
   ```

   Replace `<port>` with the port number on which the relay node will listen for incoming connections.
   Replace `<seed>` with a seed value used to generate a deterministic peer ID for the relay node.

2. The relay node will start listening for incoming connections.
   It will print the listening address once it is ready.

3. Connect other **libp2p** nodes to the relay node by specifying the relay's listening address as one of the bootstrap nodes in their configuration.

4. Once the connections are established, the relay node will facilitate communication between the connected peers, allowing them to exchange messages and data.

## Conclusion

The **libp2p** relay example demonstrates how to implement a relay node.
By running a relay node and connecting other **libp2p** nodes to it, users can create a decentralized network where peers can communicate and interact with each other.
