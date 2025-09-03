# Generic (stream) protocols

This module provides a generic [`NetworkBehaviour`](libp2p_swarm::NetworkBehaviour) for stream-oriented protocols.
Streams are the fundamental primitive of libp2p and all other protocols are implemented using streams.
In contrast to other [`NetworkBehaviour`](libp2p_swarm::NetworkBehaviour)s, this module takes a different design approach.
All interaction happens through a [`Control`] that can be obtained via [`Behaviour::new_control`].
[`Control`]s can be cloned and thus shared across your application.

## Inbound

To accept streams for a particular [`StreamProtocol`](libp2p_swarm::StreamProtocol) using this module, use [`Control::accept`]:

### Example

```rust,no_run
# fn main() {
# use libp2p_swarm::{Swarm, StreamProtocol};
# use libp2p_stream as stream;
# use futures::StreamExt as _;
let mut swarm: Swarm<stream::Behaviour> = todo!();

let mut control = swarm.behaviour().new_control();
let mut incoming = control.accept(StreamProtocol::new("/my-protocol")).unwrap();

let handler_future = async move {
    while let Some((peer, stream)) = incoming.next().await {
        // Execute your protocol using `stream`.
    }
};
# }
```

### Resource management

[`Control::accept`] returns you an instance of [`IncomingStreams`].
This struct implements [`Stream`](futures::Stream) and like other streams, is lazy.
You must continuously poll it to make progress.
In the example above, this taken care of by using the [`StreamExt::next`](futures::StreamExt::next) helper.

Internally, we will drop streams if your application falls behind in processing these incoming streams, i.e. if whatever loop calls `.next()` is not fast enough.

### Drop

As soon as you drop [`IncomingStreams`], the protocol will be de-registered.
Any further attempt by remote peers to open a stream using the provided protocol will result in a negotiation error.

## Outbound

To open a new outbound stream for a particular protocol, use [`Control::open_stream`].

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