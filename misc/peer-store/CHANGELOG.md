## 0.1.0

- Remove the full peer record and emit address removal events when a dial fails with
  `DialError::LocalPeerId`.
- Introduce `libp2p-peer-store`.
  See [PR 5724](https://github.com/libp2p/rust-libp2p/pull/5724).
