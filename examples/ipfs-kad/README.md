## Description

This example showcases the usage of **libp2p** to interact with the Kademlia protocol on the IPFS network.
The code demonstrates how to: 
 - perform Kademlia queries to find the closest peers to a specific peer ID
 - insert PK records into the Kademlia DHT

By running this example, users can gain a better understanding of how the Kademlia protocol operates and performs operations on the IPFS network.

## Usage

The example code demonstrates how to perform Kademlia queries on the IPFS network using the Rust P2P Library.

### Getting closest peers

By specifying a peer ID as a parameter, the code will search for the closest peers to the given peer ID.

#### Parameters

Run the example code:

```sh
cargo run -- get-peers [PEER_ID]
```

Replace `[PEER_ID]` with the base58-encoded peer ID you want to search for.
If no peer ID is provided, a random peer ID will be generated.

#### Example Output

Upon running the example code, you will see the output in the console.
The output will display the result of the Kademlia query, including the closest peers to the specified peer ID.

#### Successful Query Output

If the Kademlia query successfully finds closest peers, the output will be:

```sh
Searching for the closest peers to [PEER_ID]
Query finished with closest peers: [peer1, peer2, peer3]
```

#### Failed Query Output

If the Kademlia query times out or there are no reachable peers, the output will indicate the failure:

```sh
Searching for the closest peers to [PEER_ID]
Query finished with no closest peers.
```

### Inserting PK records into the DHT

By specifying `put-pk-record` as a subcommand, the code will insert the generated public key as a PK record into the DHT.

#### Parameters

Run the example code:

```sh
cargo run -- put-pk-record
```

#### Example Output

Upon running the example code, you will see the output in the console.
The output will display the result of the Kademlia operation.

#### Successful Operation Output

If the Kademlia query successfully finds closest peers, the output will be:

```sh
Putting PK record into the DHT
Successfully inserted the PK record
```

#### Failed Query Output

If the Kademlia operation times out or there are no reachable peers, the output will indicate the failure:

```sh
Putting PK record into the DHT
Failed to insert the PK record
```

## Conclusion

In conclusion, this example provides a practical demonstration of using the Rust P2P Library to interact with the Kademlia protocol on the IPFS network.
By examining the code and running the example, users can gain insights into the inner workings of Kademlia and how it performs various basic actions like getting the closest peers or inserting records into the DHT.
This knowledge can be valuable when developing peer-to-peer applications or understanding decentralized networks.
