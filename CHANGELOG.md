# Version 0.4.0 (2019-02-20)

- Modified the functions in `upgrade::transfer` to be more convenient to use.
- Now properly sending external addresses in the identify protocol.
- Fixed duplicate addresses being reported in identify and Kademlia.
- Fixed infinite looping in the functions in `upgrade::transfer`.
- Fixed infinite loop on graceful node shutdown with the `ProtocolsHandlerSelect`.
- Added a `BandwidthLogging` transport wrapper that logs the bandwidth consumption.
- The addresses to try dialing when dialing a node is now refreshed by the `Swarm` when necessary.
- Lots of modifications to the semi-private structs in `core/nodes`.
- Added `IdentifyEvent::SendBack`, when we send back our information.
- Rewrote the `MemoryTransport` to be similar to the `TcpConfig`.

# Version 0.3.1 (2019-02-02)

- Added `NetworkBehaviour::inject_replaced` that is called whenever we replace a connection with a different connection to the same peer.
- Fixed various issues with Kademlia.

# Version 0.3.0 (2019-01-30)

- Removed the `topology` module and everything it contained, including the `Topology` trait.
- Added `libp2p-noise` that supports Noise handshakes, as an alternative to `libp2p-secio`.
- Updated `ring` to version 0.14.
- Creating a `Swarm` now expects the `PeerId` of the local node, instead of a `Topology`.
- Added `NetworkBehaviour::addresses_of_peer` that returns the addresses a `NetworkBehaviour` knows about a given peer. This exists as a replacement for the topology.
- The `Kademlia` and `Mdns` behaviours now report and store the list of addresses they discover.
- You must now call `Floodsub::add_node_to_partial_view()` and `Floodsub::remove_node_from_partial_view` to add/remove nodes from the list of nodes that floodsub must send messages to.
- Added `NetworkBehaviour::inject_dial_failure` that is called when we fail to dial an address.
- `ProtocolsHandler::connection_keep_alive()` now returns a `KeepAlive` enum that provides more fine grained control.
- The `NodeHandlerWrapper` no longer has a 5 seconds inactivity timeout. This is now handled entirely by `ProtocolsHandler::connection_keep_alive()`.
- Now properly denying connections incoming from the same `PeerId` as ours.
- Added a `SwarmBuilder`. The `incoming_limit` method lets you configure the number of simultaneous incoming connections.
- Removed `FloodsubHandler`, `PingListenHandler` and `PeriodicPingHandler`.
- The structs in `core::nodes` are now generic over the `PeerId`.
- Added `SecioKeypair::ed25519_raw_key()`.
- Fix improper connection shutdown in `ProtocolsHandler`.

# Version 0.2.2 (2019-01-14)

- Fixed improper dependencies versions causing deriving `NetworkBehaviour` to generate an error.

# Version 0.2.1 (2019-01-14)

- Added the `IntoNodeHandler` and `IntoProtocolsHandler` traits, allowing node handlers and protocol handlers to know the `PeerId` of the node they are interacting with.

# Version 0.2 (2019-01-10)

- The `Transport` trait now has an `Error` associated type instead of always using `std::io::Error`.
- Merged `PeriodicPing` and `PingListen` into one `Ping` behaviour.
- `Floodsub` now generates `FloodsubEvent`s instead of direct floodsub messages.
- Added `ProtocolsHandler::connection_keep_alive`. If all the handlers return `false`, then the connection to the remote node will automatically be gracefully closed after a few seconds.
- The crate now successfuly compiles for the `wasm32-unknown-unknown` target.
- Updated `ring` to version 0.13.
- Updated `secp256k1` to version 0.12.
- The enum returned by `RawSwarm::peer()` can now return `LocalNode`. This makes it impossible to accidentally attempt to dial the local node.
- Removed `Transport::map_err_dial`.
- Removed the `Result` from some connection-related methods in the `RawSwarm`, as they could never error.
- If a node doesn't respond to pings, we now generate an error on the connection instead of trying to gracefully close it.
