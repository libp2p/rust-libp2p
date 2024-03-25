## Description

This example shows the usage of the `stream::Behaviour`.
As a counter-part to the `request_response::Behaviour`, the `stream::Behaviour` allows users to write stream-oriented protocols whilst having minimal interaction with the `Swarm`.

In this showcase, we implement an echo protocol: All incoming data is echoed back to the dialer, until the stream is closed.

## Usage

To run the example, follow these steps:

1. Start an instance of the example in one terminal:

   ```sh
   cargo run --bin stream-example
   ```

   Observe printed listen address.

2. Start another instance in a new terminal, providing the listen address of the first one.

   ```sh
   cargo run --bin stream-example -- <address>
   ```

3. Both terminals should now continuously print messages. 

## Conclusion

The `stream::Behaviour` is an "escape-hatch" from the way typical rust-libp2p protocols are written.
It is suitable for several scenarios including:

- prototyping of new protocols
- experimentation with rust-libp2p
- integration in `async/await`-heavy applications