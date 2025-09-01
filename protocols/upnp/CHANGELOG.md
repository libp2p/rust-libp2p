## 0.5.1

- Skip port mapping when an active port mapping is present.
  Previously, the behavior would skip creating new mappings if any mapping 
  (active or inactive or pending) existed for the same port. Now it correctly only 
  checks active mappings on the gateway.
  See [PR 6127](https://github.com/libp2p/rust-libp2p/pull/6127).

- Fix excessive retry attempts for failed port mappings by implementing exponential backoff.
  Failed mappings now retry up to 5 times with increasing delays (30s to 480s) before giving up.
  This prevents continuous retry loops.
  See [PR 6128](https://github.com/libp2p/rust-libp2p/pull/6128).

## 0.5.0

- update igd-next to 0.16.1
  See [PR 5944](https://github.com/libp2p/rust-libp2p/pull/5944).

- Fix panic during a shutdown process.
  See [PR 5998](https://github.com/libp2p/rust-libp2p/pull/5998).

<!-- Update to libp2p-swarm v0.47.0 -->

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
