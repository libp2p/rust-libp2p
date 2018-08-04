// ### Leaving the Overlay

// In order to unsubscribe, the node can just leave the overlay by
// sending `DISCONNECT` messages to its active neighbors.  References to
// the node in the various passive lists scattered across the overlay
// will be lazily pruned over time by the passive view management
// component of the protocol.

// Optimization TODO: In order to facilitate fast clean up of departing nodes, we can also
// introduce a `LEAVE` message that eagerly propagates across the
// network.  A node that wants to unsubscribe from the topic, emits a
// `LEAVE` to its active list neighbors in place of `DISCONNECT`.  Upon
// receiving a `LEAVE`, a node removes the node from its active list
// _and_ passive lists. If the node was removed from one of the lists or
// if the TTL is greater than zero, then the `LEAVE` is propagated
// further across the active list links. This will ensure a random
// diffusion through the network that would clean most of the active
// lists eagerly, at the cost of some bandwidth.