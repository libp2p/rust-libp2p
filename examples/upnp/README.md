## Description

The upnp example showcases how to use the upnp network behaviour to externally open ports on the network gateway.


## Usage

To run the example, follow these steps:

1. In a terminal window, run the following command:

   ```sh
   cargo run
   ```

2. This command will start the swarm and print the `NewExternalAddr` if the gateway supports `UPnP` or
   `GatewayNotFound` if it doesn't.


## Conclusion

The upnp example demonstrates the usage of **libp2p** to externally open a port on the gateway if it
supports [`UPnP`](https://en.wikipedia.org/wiki/Universal_Plug_and_Play).
