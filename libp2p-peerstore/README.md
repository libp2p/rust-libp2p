The `peerstore` crate allows one to store information about a peer.

`peerstore` is a key-value database, where the keys are multihashes (which usually corresponds
to the hash of the public key of the peer, but that is not enforced by this crate) and the
values are the public key and a list of multiaddresses. Contrary a regular key-value storage,
the multiaddresses stored by the `peerstore` have a time-to-live after which they disappear.

This crate consists in a generic `Peerstore` trait and the follow implementations:

- `JsonPeerstore`: Stores the information in a single JSON file.
- `MemoryPeerstore`: Stores the information in memory.

Note that the peerstore implementations do not consider information inside a peer store to be
critical. In case of an error (eg. corrupted file, disk error, etc.) they will prefer to lose
data rather than returning the error.

# Example

```rust
extern crate multiaddr;
extern crate libp2p_peerstore;

use libp2p_peerstore::memory_peerstore::MemoryPeerstore;
use libp2p_peerstore::{Peerstore, PeerAccess};
use multiaddr::Multiaddr;
use std::time::Duration;

// In this example we use a `MemoryPeerstore`, but you can easily swap it for another backend.
let mut peerstore = MemoryPeerstore::empty();
let peer_id = vec![1, 2, 3, 4];

// Let's write some information about a peer.
{
    // `peer` mutably borrows the peerstore, so we have to do it in a local scope.
    let mut peer = peerstore.peer_or_create(&peer_id);
    peer.set_pub_key(vec![60, 90, 120, 150]);
    peer.add_addr(Multiaddr::new("/ip4/10.11.12.13/tcp/20000").unwrap(),
                Duration::from_millis(5000));
}

// Now let's load back the info.
{
    let mut peer = peerstore.peer(&peer_id).expect("peer doesn't exist in the peerstore");
    println!("pub key = {:?}", peer.get_pub_key());     // should be [60, 90, 120, 150]
    println!("addresses = {:?}", peer.addrs().collect::<Vec<_>>());
}
```
