This crate provides the `RwStreamSink` type. It wraps around a `Stream + Sink` that produces
byte arrays, and implements `AsyncRead` and `AsyncWrite`.

Each call to `write()` will send one packet on the sink. Calls to `read()` will read from
incoming packets.

> **Note**: This crate is mostly a utility crate and is not at all specific to libp2p.
