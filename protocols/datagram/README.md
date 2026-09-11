# libp2p-datagram

Unreliable datagrams over libp2p connections, per [libp2p/specs#680].

Datagrams ride a QUIC `/dg/1` control stream that binds them to one application
protocol. They may be dropped, reordered, or duplicated and carry no flow
control; the caller owns reliability. Only QUIC carries them today; sends on
other transports fail with `SendError`.

```rust,no_run
use libp2p_datagram as datagram;
use libp2p_swarm::StreamProtocol;

let mut behaviour = datagram::Behaviour::new(StreamProtocol::new("/my/app/1.0.0"));
let mut control = behaviour.new_control();
let mut incoming = behaviour.incoming_datagrams().unwrap();

// add `behaviour` to your Swarm, then:
// control.send_datagram(peer, bytes)?;
// while let Some((from, bytes)) = incoming.next().await { ... }
```

## License

Licensed under MIT.

[libp2p/specs#680]: https://github.com/libp2p/specs/pull/680
