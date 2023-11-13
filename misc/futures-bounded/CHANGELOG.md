## 0.2.2 - unreleased

- Introduce `FuturesTupleSet`, holding tuples of a `Future` together with an arbitrary piece of data.
  See [PR 4841](https://github.com/libp2p/rust-lib2pp/pulls/4841).

## 0.2.1

- Add `.len()` getter to `FuturesMap`, `FuturesSet`, `StreamMap` and `StreamSet`.
  See [PR 4745](https://github.com/libp2p/rust-lib2pp/pulls/4745).

## 0.2.0

- Add `StreamMap` type and remove `Future`-suffix from `PushError::ReplacedFuture` to reuse it for `StreamMap`.
  See [PR 4616](https://github.com/libp2p/rust-lib2pp/pulls/4616).

## 0.1.0

Initial release.
