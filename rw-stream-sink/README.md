This crate provides the `RwStreamSink` type. It wraps around a `Stream + Sink` that produces
and accepts byte arrays, and implements `AsyncRead` and `AsyncWrite`.

Each call to `write()` will send one packet on the sink. Calls to `read()` will read from
incoming packets.

> **Note**: Although this crate is hosted in the libp2p repo, it is purely a utility crate and
>           not at all specific to libp2p.
