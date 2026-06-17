# libp2p-datagram

Unreliable datagrams over libp2p connections.

Datagrams may be dropped, reordered, or duplicated and carry no flow control;
the caller owns reliability. Only QUIC carries them today; sends on other
transports fail with `SendError`.

```rust,no_run
use libp2p_datagram as datagram;

let mut behaviour = datagram::Behaviour::new();
let mut control = behaviour.new_control();
let mut incoming = behaviour.incoming_datagrams().unwrap();

// add `behaviour` to your Swarm, then:
// control.send_datagram(peer, bytes)?;
// while let Some((from, bytes)) = incoming.next().await { ... }
```

## License

Licensed under MIT.
