This crate provides the `RwStreamSink` type. It wraps around a `Stream`
and `Sink` that produces and accepts byte arrays, and implements the
`AsyncRead` and `AsyncWrite` traits.

Each call to `AsyncWrite::poll_write` will send one packet to the sink.
Calls to `AsyncRead::poll_read` will read from the stream's incoming packets.
