## Description

The "Direct Connection Upgrade through Relay" (DCUTR) protocol allows peers in a peer-to-peer network to establish direct connections with each other.
This example provides a basic usage of this protocol in **libp2p**.

## Usage

To run the example, follow these steps:

1. Run the example using Cargo:
   ```sh
   cargo run -- <OPTIONS>
   ```
   Replace `<OPTIONS>` with specific options.

### Available Options

The following options can be used with the example:

- `--mode <MODE>`: Specify the mode of operation for the example. Possible values are `dial` (client-dial) or `listen` (client-listen).
- `--secret-key-seed <SEED>`: Specify a fixed value to generate a deterministic peer ID.
- `--relay-address <ADDRESS>`: Specify the listening address for the relay server.
- `--remote-peer-id <ID>`: Specify the Peer ID of the remote peer to establish a connection with (only in `dial` mode).

### Example usage

- Example usage in client-dial mode:
	```sh
	cargo run -- --mode dial --secret-key-seed 42 --relay-address /ip4/127.0.0.1/tcp/12345 --remote-peer-id <REMOTE_PEER_ID>
	```

- Example usage in client-listen mode:
	```sh
	cargo run -- --mode listen --secret-key-seed 42 --relay-address /ip4/127.0.0.1/tcp/12345
	```

## Conclusion

The DCUTR protocol offers a solution for achieving direct connectivity between peers in a peer-to-peer network.
By utilizing hole punching and eliminating the need for signaling servers, the protocol allows peers behind NATs to establish direct connections.
This example provides instructions on running an example implementation of the protocol, allowing users to explore its functionality and benefits.
By adopting this protocol, peer-to-peer networks can enhance their efficiency and reduce reliance on costly relay infrastructure.
