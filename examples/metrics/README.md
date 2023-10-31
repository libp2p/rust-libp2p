## Description

The example showcases how to run a p2p network with **libp2p** and collect metrics using `libp2p-metrics` as well as span data via `opentelemetry`.
It sets up multiple nodes in the network and measures various metrics, such as `libp2p_ping`, to evaluate the network's performance.

## Usage

To run the example, follow these steps:

1. Run the following command to start the first node:

   ```sh
   cargo run
   ```

2. Open a second terminal and run the following command to start a second node:

   ```sh
   cargo run -- <listen-addr-of-first-node>
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

Then, configure tracing to output spans:

```shell
export RUST_LOG=info,[ConnectionHandler::poll]=trace,[NetworkBehaviour::poll]=trace
```

Next, (re)-start the two example for it to connect to the OTEL collector.
Finally, open the Jaeger UI in a browser and explore the spans: http://localhost:16686.

### Filtering spans

For a precise documentation, please see the following documentation in tracing: <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#directives>.

`rust-libp2p` consistently applies spans to the following functions:

- `ConnectionHandler::poll` implementations
- `NetworkBehaviour::poll` implementations

The above spans are all called exactly that: `ConnectionHandler::poll` and `NetworkBehaviour::poll`.
You can activate _all_ of them by setting:

```
RUST_LOG=[ConnectionHandler::poll]=trace
```

If you just wanted to see the spans of the `libp2p_ping` crate, you can filter like this:

```
RUST_LOG=libp2p_ping[ConnectionHandler::poll]=trace
```

## Conclusion

This example demonstrates how to utilize the `libp2p-metrics` crate to collect and analyze metrics in a libp2p network.
By running multiple nodes and examining the metrics, users can gain insights into the network's performance and behavior.
