## Description

This example showcases a minimal implementation of a **libp2p** node that can interact with IPFS.
It utilizes the gossipsub protocol for pubsub messaging, the ping protocol for network connectivity testing, and the identify protocol for peer identification.
The node can be used to communicate with other IPFS nodes that have gossipsub enabled.

To establish a connection with other nodes, you can provide their multiaddresses as command-line arguments.
On startup, the example will display a list of addresses that you can dial from a `go-ipfs` or `js-ipfs` node.

## Usage

To run the example, follow these steps:

1.  Build and run the example using Cargo:
    ```sh
    cargo run [ADDRESS_1] [ADDRESS_2] ...
    ```

    Replace `[ADDRESS_1]`, `[ADDRESS_2]`, etc., with the multiaddresses of the nodes you want to connect to.
    You can provide multiple addresses as command-line arguments.

    **Note:** The multiaddress should be in the following format: `/ip4/127.0.0.1/tcp/4001/p2p/peer_id`.

2.  Once the example is running, you can interact with the IPFS node using the following commands:

    -   **Pubsub (Gossipsub):** You can use the gossipsub protocol to send and receive messages on the "chat" topic.
        To send a message, type it in the console and press Enter.
        The message will be broadcasted to other connected nodes using gossipsub.

    -   **Ping:** You can ping other connected nodes to test network connectivity.
        The example will display the round-trip time (RTT) for successful pings or indicate if a timeout occurs.


## Conclusion

This example provides a basic implementation of an IPFS node using **libp2p**.
It demonstrates the usage of the gossipsub, ping, and identify protocols to enable communication with other IPFS nodes.
By running this example and exploring its functionality, you can gain insights into how to build more advanced P2P applications using Rust.

Feel free to experiment with different multiaddresses and explore the capabilities of **libp2p** in the context of IPFS. Happy coding!
