# Generic stream protocols

This module provides a generic [`NetworkBehaviour`](libp2p_swarm::NetworkBehaviour) for stream-oriented protocols.

## Inbound

To accept streams for a particular [`StreamProtocol`] using this module, use [`Behaviour::accept`]:

### Example

```rust,no_run
# fn main() {
# use libp2p_swarm::{Swarm, StreamProtocol};
# use libp2p_stream as stream;
# use futures::StreamExt as _;
let mut swarm: Swarm<stream::Behaviour> = todo!();

let mut incoming = swarm.behaviour_mut().accept(StreamProtocol::new("/my-protocol")).unwrap();

let handler_future = async move {
    while let Some((peer, stream)) = incoming.next().await {
        // Execute your protocol using `stream`.
    }
};
# }
```

### Backpressure

[`Behaviour::accept`] returns you an instance of [`IncomingStreams`].
This struct implements [`Stream`](futures::Stream) and like other streams, is lazy.
You must continuously poll it to make progress.
In the example above, this taken care of by using the [`StreamExt::next`](futures::StreamExt::next) helper.

Internally, we will drop streams if your application falls behind in processing these incoming streams, i.e. if whatever loop calls `.next()` is not fast enough.

### Drop

As soon as you drop [`IncomingStreams`], the protocol will be de-registered.
Any further attempt by remote peers to open a stream using the provided protocol will result in a negotiation error.

## Outbound

To open a new outbound stream for a particular protocol, you can obtain a [`Control`] using [`Behaviour::new_control`].
In contrast to [`IncomingStreams`]s, [`Control`]s can be cloned and you can obtain as many [`Control`]s for the same protocol as you want.

To open a stream, you need construct a [`PeerControl`] using [`Control::peer`].
This function is `async` and will block until we have a connection to the given peer.

### Example

```rust,no_run
# fn main() {
# use libp2p_swarm::{Swarm, StreamProtocol};
# use libp2p_stream as stream;
# use libp2p_identity::PeerId;
let mut swarm: Swarm<stream::Behaviour> = todo!();
let peer_id: PeerId = todo!();

let mut control = swarm.behaviour().new_control();

let protocol_future = async move {
    let stream = control.open_stream(peer_id, StreamProtocol::new("/my-protocol")).await.unwrap();

    // Execute your protocol here using `stream`.
};
# }
```