# Version 0.2

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
