## Description

This example showcases a basic distributed key-value store implemented using **libp2p**, along with the mDNS and Kademlia protocols.

## Usage

### Key-Value Store

1.  Open two terminal windows, type `cargo run` and press Enter.

2.  In terminal one, type `PUT my-key my-value` and press Enter.
    This command will store the value `my-value` with the key `my-key` in the distributed key-value store.

3.  In terminal two, type `GET my-key` and press Enter.
    This command will retrieve the value associated with the key `my-key` from the key-value store.

4.  To exit, press `Ctrl-c` in each terminal window to gracefully close the instances.


### Provider Records

You can also use provider records instead of key-value records in the distributed store.

1.  Open two terminal windows and start two instances of the key-value store.
    If your local network supports mDNS, the instances will automatically connect.

2.  In terminal one, type `PUT_PROVIDER my-key` and press Enter.
    This command will register the peer as a provider for the key `my-key` in the distributed key-value store.

3.  In terminal two, type `GET_PROVIDERS my-key` and press Enter.
    This command will retrieve the list of providers for the key `my-key` from the key-value store.

4.  To exit, press `Ctrl-c` in each terminal window to gracefully close the instances.


Feel free to explore and experiment with the distributed key-value store example, and observe how the data is distributed and retrieved across the network using **libp2p**, mDNS, and the Kademlia protocol.

## Conclusion

This example demonstrates the implementation of a basic distributed key-value store using **libp2p**, mDNS, and the Kademlia protocol.
By leveraging these technologies, peers can connect, store, and retrieve key-value pairs in a decentralized manner.
The example provides a starting point for building more advanced distributed systems and exploring the capabilities of **libp2p** and its associated protocols.
