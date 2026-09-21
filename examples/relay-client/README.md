## Description

A small relay client that demonstrates the autorelay.

## Run the client

In another terminal, pointing it at the relay:

```
cargo run \
    --secret-key-seed 1 \
    --relay /ip4/$RELAY_IP/tcp/$PORT/p2p/$RELAY_PEERID \
    --max-reservations 2
```

Provide `relay` multiple times to point at additional relays; autorelay will
pick among them up to `max-reservations`.