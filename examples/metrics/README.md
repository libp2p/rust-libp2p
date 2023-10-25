## Description

The example showcases how to run a p2p network with **libp2p** and collect metrics using `libp2p-metrics` as well as span data via `opentelemetry`.
It sets up multiple nodes in the network and measures various metrics, such as `libp2p_ping`, to evaluate the network's performance.

## Usage

To run the example, follow these steps:

1. Run the following command to start the first node:

   ```sh
   RUST_LOG=info cargo run
   ```

2. Open a second terminal and run the following command to start a second node:

   ```sh
   RUST_LOG=info cargo run -- <listen-addr-of-first-node>
   ```

   Replace `<listen-addr-of-first-node>` with the listen address of the first node reported in the first terminal.
   Look for the line that says `NewListenAddr` to find the address.

3. Open a third terminal and run the following command to retrieve the metrics from either the first or second node:

   ```sh
   curl localhost:<metrics-port-of-first-or-second-node>/metrics
   ```

   Replace `<metrics-port-of-first-or-second-node>` with the listen port of the metrics server of either the first or second node.
   Look for the line that says `tide::server Server listening on` to find the port.

   After executing the command, you should see a long list of metrics printed to the terminal.
   Make sure to check the `libp2p_ping` metrics, which should have a value greater than zero (`>0`).

## Opentelemetry

To see the span data collected as part of the `Swarm`s activity, start up an opentelemetry collector:

```sh
docker compose up
```



## Conclusion

This example demonstrates how to utilize the `libp2p-metrics` crate to collect and analyze metrics in a libp2p network.
By running multiple nodes and examining the metrics, users can gain insights into the network's performance and behavior.
