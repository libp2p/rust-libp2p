## Description

The ping example showcases how to create a network of nodes that establish connections, negotiate the ping protocol, and ping each other.

## Usage

To run the example, follow these steps:

1. In a first terminal window, run the following command:

   ```sh
   cargo run
   ```

   This command starts a node and prints the `PeerId` and the listening addresses, such as `Listening on "/ip4/0.0.0.0/tcp/24915"`.

2. In a second terminal window, start a new instance of the example with the following command:

   ```sh
   cargo run -- /ip4/127.0.0.1/tcp/24915
   ```

   Replace `/ip4/127.0.0.1/tcp/24915` with the listen address of the first node obtained from the first terminal window.

3. The two nodes will establish a connection, negotiate the ping protocol, and begin pinging each other.

## Conclusion

The ping example demonstrates the basic usage of **libp2p** to create a simple p2p network and implement a ping protocol.
By running multiple nodes and observing the ping behavior, users can gain insights into how **libp2p** facilitates communication and interaction between peers.
