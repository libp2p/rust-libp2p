## Description

The "Direct Connection Upgrade through Relay" (DCUTR) protocol allows peers in a peer-to-peer network to establish direct connections with each other.
In other words, DCUTR is libp2p's version of hole-punching.
This example provides a basic usage of this protocol in **libp2p**.

## Usage

To run the example, follow these steps:

1. Run the example using Cargo:
   ```sh
   cargo run -- <OPTIONS>
   ```
   Replace `<OPTIONS>` with specific options (you can use the `--help` command to see the available options).

### Example usage

- Example usage in client-listen mode:
	```sh
	cargo run -- --mode listen --secret-key-seed 42 --relay-address /ip4/$RELAY_IP/tcp/$PORT/p2p/$RELAY_PEERID
	```

- Example usage in client-dial mode:
	```sh
	cargo run -- --mode dial --secret-key-seed 42 --relay-address /ip4/$RELAY_IP/tcp/$PORT/p2p/$RELAY_PEERID --remote-peer-id <REMOTE_PEER_ID>
	```

For this example to work, it is also necessary to turn on a relay server (you will find the related instructions in the example in the `examples/relay-server` folder).

## Conclusion

The DCUTR protocol offers a solution for achieving direct connectivity between peers in a peer-to-peer network.
By utilizing hole punching and eliminating the need for signaling servers, the protocol allows peers behind NATs to establish direct connections.
This example provides instructions on running an example implementation of the protocol, allowing users to explore its functionality and benefits.
