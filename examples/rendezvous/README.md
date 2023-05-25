## Description

The rendezvous protocol example showcases how to implement a rendezvous server and interact with it using different binaries.
The rendezvous server facilitates peer registration and discovery, enabling peers to find and communicate with each other in a decentralized manner.

## Usage

To run the example, follow these steps:

1. Start the rendezvous server by running the following command:

   ```sh
   RUST_LOG=info cargo run --bin rendezvous-example
   ```

   This command starts the rendezvous server, which will listen for incoming connections and handle peer registrations and discovery.

2. Register a peer by executing the following command:

   ```sh
   RUST_LOG=info cargo run --bin rzv-register
   ```

   This command registers a peer with the rendezvous server, allowing the peer to be discovered by other peers.

3. Try to discover the registered peer from the previous step by running the following command:

   ```sh
   RUST_LOG=info cargo run --bin rzv-discover
   ```

   This command attempts to discover the registered peer using the rendezvous server.
   If successful, it will print the details of the discovered peer.

4. Additionally, you can try discovering a peer using the identify protocol by executing the following command:

   ```sh
   RUST_LOG=info cargo run --bin rzv-identify
   ```

   This command demonstrates peer discovery using the identify protocol.
   It will print the peer's identity information if successful.

5. Experiment with different registrations, discoveries, and combinations of protocols to explore the capabilities of the rendezvous protocol and libp2p library.

## Conclusion

The rendezvous protocol example provides a practical demonstration of how to implement peer registration and discovery using **libp2p**.
By running the rendezvous server and utilizing the provided binaries, users can register peers and discover them in a decentralized network.

Feel free to explore the code and customize the behavior of the rendezvous server and the binaries to suit your specific use cases.
