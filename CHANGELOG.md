# Version 0.10.0 (2019-06-25)

- `PollParameters` is now a trait instead of a struct.
- The `Swarm` can now be customized with connection information.
- Fixed write-only substreams now delivering data properly.
- Fixed the TCP listener accidentally shutting down if an incoming socket was closed too quickly.
- Improved the heuristics for determining external multiaddresses based on reports.
- Various fixes to Kademlia iterative queries and the WebSockets transport.

# Version 0.9.1 (2019-06-05)

- `EitherOutput` now implements `Stream` and `Sink` if their variants also implement these traits.
- `libp2p::websocket::error::Error` now implements `Sync`.

# Version 0.9.0 (2019-06-04)

- Major fixes and performance improvements to libp2p-kad.
- Initial prototype for record storage in libp2p-kad.
- Rewrote the implementation of WebSockets. It now properly supports WebSockets Secure (WSS).
- Removed `BrowserWsConfig`. Please use `libp2p::wasm_ext::ExtTransport` instead.
- Added a `Path` parameter to `multiaddr::Protocol::WS` and `WSS`. The string representation when a path is present is respectively `x-parity-ws/<path>` and `x-parity-wss/<path>` where `<path>` is percent-encoded.
- Fixed an issue with `libp2p-tcp` where the wrong listened address was returned, if the actual address was loopback.
- Added `core::upgrade::OptionalUpgrade`.
- Added some utility functions in `core::identity::secp256k1`.
- It is now possible to inject an artificial connection in the `RawSwarm`.

# Version 0.8.1 (2019-05-15)

- Fixed a vulnerability in ED25519 signatures verification in libp2p-core.

# Version 0.8.0 (2019-05-15)

- Crate now successfully runs from within the browser when compiled to WASM.
- Modified the constructors of `NoiseConfig` to accept any type of public key. The Noise handshake has consequently been modified.
- Changed the `StreamMuxer` trait to have an `Error` associated type.
- The `Swarm` now ranks externally-visible multiaddresses by how often they have been reported, ensuring that weird or malicious reports don't affect connectivity too much.
- Added `IntoProtocolsHandler::inbound_protocol`. Must return the same value as what `ProtocolsHandler::listen_protocol` would return.
- `IntoProtocolsHandler::into_handler` now takes a second parameter with the `&ConnectedPoint` to the node we are connected to.
- Replaced the `secp256k1` crate with `libsecp256k1`.
- Fixed `Kademlia::add_providing` taking a `PeerId` instead of a `Multihash`.
- Fixed various bugs in the implementation of `Kademlia`.
- Added `OneSubstreamMuxer`.
- Added the `libp2p-wasm-ext` crate.
- Added `multiaddr::from_url`.
- Added `OptionalTransport`.

# Version 0.7.1 (2019-05-15)

- Fixed a vulnerability in ED25519 signatures verification in libp2p-core.

# Version 0.7.0 (2019-04-23)

- Fixed the inactive connections shutdown mechanism not working.
- `Transport::listen_on` must now return a `Stream` that produces `ListenEvent`s. This makes it possible to notify about listened addresses at a later point in time.
- `Transport::listen_on` no longer returns an address we're listening on. This is done through `ListenEvent`s. All other `listen_on` methods have been updated accordingly.
- Added `NetworkBehaviour::inject_new_listen_addr`, `NetworkBehaviour::inject_expired_listen_addr` and `NetworkBehaviour::inject_new_external_addr`.
- `ProtocolsHandler::listen_protocol` and `ProtocolsHandlerEvent::OutboundSubstreamRequest` must now return a `SubstreamProtocol` struct containing a timeout for the upgrade.
- `Ping::new` now requires a `PingConfig`, which can be created with `PingConfig::new`.
- Removed `Transport::nat_traversal` in favour of a stand-alone `address_translation` function in `libp2p-core`.
- Reworked the API of `Multiaddr`.
- Removed the `ToMultiaddr` trait in favour of `TryFrom`.
- Added `Swarm::ban_peer_id` and `Swarm::unban_peer_id`.
- The `TPeerId` generic parameter of `RawSwarm` is now `TConnInfo` and must now implement a `ConnectionInfo` trait.
- Reworked the `PingEvent`.
- Renamed `KeepAlive::Forever` to `Yes` and `KeepAlive::Now` to `No`.

# Version 0.6.0 (2019-03-29)

- Replaced `NetworkBehaviour::inject_dial_failure` with `inject_dial_failure` and
  `inject_addr_reach_failure`. The former is called when we have finished trying to dial a node
  without success, while the latter is called when we have failed to reach a specific address.
- Fixed Kademlia storing a different hash than the reference implementation.
- Lots of bugfixes in Kademlia.
- Modified the `InboundUpgrade` and `OutboundUpgrade` trait to take a `Negotiated<TSocket>` instead
  of `TSocket`.
- `PollParameters::external_addresses` now returns `Multiaddr`es as reference instead of by value.
- Added `Swarm::external_addresses`.
- Added a `core::swarm::toggle::Toggle` that allows having a disabled `NetworkBehaviour`.

# Version 0.5.0 (2019-03-13)

- Moved the `SecioKeypair` struct in `core/identity` and renamed it to `Keypair`.
- mplex now supports half-closed substreams.
- Renamed `StreamMuxer::shutdown()` to `close()`.
- Closing a muxer with the `close()` method (formerly `shutdown`) now "destroys" all the existing substreams. After `close()` as been called, they all return either EOF or an error.
- The `shutdown_substream()` method now closes only the writing side of the substream, and you can continue reading from it until EOF or until you delete it. This was actually already more or less the case before, but it wasn't properly reflected in the API or the documentation.
- `poll_inbound()` and `poll_outbound()` no longer return an `Option`, as `None` was the same as returning an error.
- Removed the `NodeClosed` events and renamed `NodeError` to `NodeClosed`. From the API's point of view, a connection now always closes with an error.
- Added the `NodeHandlerWrapperError` enum that describes an error generated by the protocols handlers grouped together. It is either `UselessTimeout` or `Handler`. This allows properly reporting closing a connection because it is useless.
- Removed `NodeHandler::inject_inbound_closed`, `NodeHandler::inject_outbound_closed`, `NodeHandler::shutdown`, and `ProtocolsHandler::shutdown`. The handler is now dropped when a shutdown process starts. This should greatly simplify writing a handler.
- `StreamMuxer::close` now implies `flush_all`.
- Removed the `Shutdown` enum from `stream_muxer`.
- Removed `ProtocolsHandler::fuse()`.
- Reworked some API of `core/nodes/node.rs` and `core/nodes/handled_node.rs`.
- The core now works even outside of a tokio context.

# Version 0.4.2 (2019-02-27)

- Fixed periodic pinging not working.

# Version 0.4.1 (2019-02-20)

- Fixed wrong version of libp2p-noise.

# Version 0.4.0 (2019-02-20)

- The `multiaddr!` macro has been moved to the `multiaddr` crate and is now reexported under the name `build_multiaddr!`.
- Modified the functions in `upgrade::transfer` to be more convenient to use.
- Now properly sending external addresses in the identify protocol.
- Fixed duplicate addresses being reported in identify and Kademlia.
- Fixed infinite looping in the functions in `upgrade::transfer`.
- Fixed infinite loop on graceful node shutdown with the `ProtocolsHandlerSelect`.
- Fixed various issues with nodes dialing each other simultaneously.
- Added the `StreamMuxer::is_remote_acknowledged()` method.
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
