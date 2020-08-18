# 0.22.0 [unreleased]

- Store addresses in provider records.
  See [PR 1708](https://github.com/libp2p/rust-libp2p/pull/1708).

- Update `libp2p-core` and `libp2p-swarm` dependencies.

- Add `KBucketRef::range` exposing the minimum inclusive and maximum inclusive
  `Distance` for the bucket
  ([PR 1680](https://github.com/libp2p/rust-libp2p/pull/1680)).

- Add `NetworkBehaviour::inject_address_change` implementation
  ([PR 1649](https://github.com/libp2p/rust-libp2p/pull/1649)).

# 0.21.0 [2020-07-01]

- Remove `KademliaEvent::Discovered`
  ([PR 1632](https://github.com/libp2p/rust-libp2p/pull/1632))

- More control and insight for k-buckets
  ([PR 1628](https://github.com/libp2p/rust-libp2p/pull/1628)).
  In particular, `Kademlia::kbuckets_entries` has been removed and
  replaced by `Kademlia::kbuckets`/`Kademlia::kbucket` which provide
  more information than just the peer IDs. Furthermore `Kademlia::add_address`
  now returns a result and two new events, `KademliaEvent::RoutablePeer`
  and `KademliaEvent::PendingRoutablePeer` are introduced (but are not
  required to be acted upon in order to retain existing behaviour).
  For more details, see the PR description.

# 0.20.1 [2020-06-23]

- Maintenance release ([PR 1623](https://github.com/libp2p/rust-libp2p/pull/1623)).

# 0.20.0 [2020-06-22]

- Optionally require iterative queries to use disjoint paths based
  on S/Kademlia for increased resiliency in the presence of potentially
  adversarial nodes ([PR 1473](https://github.com/libp2p/rust-libp2p/pull/1473)).

- Re-export several types
  ([PR 1595](https://github.com/libp2p/rust-libp2p/pull/1595)).

- Updated dependencies.
