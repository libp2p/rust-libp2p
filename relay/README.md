# Circuit Relay v0.1.0

Implements the `/libp2p/circuit/relay/0.1.0` protocol [[1][1]]. It allows a source `A` to connect
to a destination `B` via an intermediate relay node `R` to which `B` is already connected to. This
is used as a last resort to make `B` reachable from other nodes when it would normally not be, e.g.
due to certain NAT setups.

[1]: https://github.com/libp2p/specs/blob/97b86236ce07ff35d9bff5c1ba60daa8879f8f03/relay/

