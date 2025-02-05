## 0.4.0

- update igd-next to 0.15.1.
  See [PR XXXX](https://github.com/libp2p/rust-libp2p/pull/XXXX).

<!-- Update to libp2p-core v0.43.0 -->

## 0.3.0

<!-- Update to libp2p-swarm v0.45.0 -->

## 0.2.2
- Fix a panic caused when `upnp::Gateway` is dropped and its events queue receiver is no longer
available.
  See [PR 5273](https://github.com/libp2p/rust-libp2p/pull/5273).

## 0.2.1
- Fix a panic caused when dropping `upnp::Behaviour` such as when used together with `Toggle`.
  See [PR 5096](https://github.com/libp2p/rust-libp2p/pull/5096).

## 0.2.0


## 0.1.1

- Fix high CPU usage due to repeated generation of failure events.
  See [PR 4569](https://github.com/libp2p/rust-libp2p/pull/4569).

- Fix port mapping protocol used for a UDP multiaddress.
  See [PR 4542](https://github.com/libp2p/rust-libp2p/pull/4542).

## 0.1.0

- Initial version
