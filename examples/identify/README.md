## Description

The example demonstrates how to create a connection between two nodes using TCP transport, authenticate with the noise protocol, and multiplex data streams with yamux.
The library provides a behavior for identity network interactions, allowing nodes to exchange identification information securely.
By running the example, the nodes will establish a connection, negotiate the identity protocol, and exchange identification information, which will be displayed in the console.

## Usage

1. In the first terminal window, run the following command:
    ```sh
    cargo run
    ```
    This will print the peer ID (`PeerId`) and the listening addresses, e.g., `Listening on "/ip4/127.0.0.1/tcp/24915"`

2. In the second terminal window, start a new instance of the example with the following command:
    ```sh
    cargo run -- /ip4/127.0.0.1/tcp/24915
    ```
    The two nodes establish a connection, negotiate the identity protocol, and send each other identification information, which is then printed to the console.

## Conclusion

The included identity example demonstrates how to establish connections and exchange identification information between nodes using the library's protocols and behaviors.
