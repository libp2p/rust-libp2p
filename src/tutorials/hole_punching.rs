// Copyright 2022 Protocol Labs.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! # Hole Punching Tutorial
//!
//! This tutorial shows hands-on how to overcome firewalls and NATs with libp2p's hole punching
//! mechanism. Before we get started, please read the [blog
//! post](https://blog.ipfs.io/2022-01-20-libp2p-hole-punching/) to familiarize yourself with libp2p's hole
//! punching mechanism on a conceptual level.
//!
//! We will be using the [Circuit Relay v2](crate::relay::v2) and the [Direct Connection
//! Upgrade through Relay (DCUtR)](crate::dcutr) protocol.
//!
//! You will need 3 machines for this tutorial:
//!
//! - A relay server:
//!    - Any public server will do, e.g. a cloud provider VM.
//! - A listening client:
//!    - Any computer connected to the internet, but not reachable from outside its own network,
//!      works.
//!    - This can e.g. be your friends laptop behind their router (firewall + NAT).
//!    - This can e.g. be some cloud provider VM, shielded from incoming connections e.g. via
//!      Linux's UFW on the same machine.
//!    - Don't use a machine that is in the same network as the dialing client. (This would require
//!      NAT hairpinning.)
//! - A dialing client:
//!    - Like the above, any computer connected to the internet, but not reachable from the outside.
//!    - Your local machine will likely fulfill these requirements.
//!
//! ## Setting up the relay server
//!
//! Hole punching requires a public relay node for the two private nodes to coordinate their hole
//! punch via. For that we need a public server somewhere in the Internet. In case you don't have
//! one already, any cloud provider VM will do.
//!
//! Either on the server directly, or on your local machine, compile the example relay server:
//!
//! ``` bash
//! ## Inside the rust-libp2p repository.
//! cargo build --example relay_v2 -p libp2p-relay
//! ```
//!
//! You can find the binary at `target/debug/examples/relay_v2`. In case you built it locally, copy
//! it to your server.
//!
//! On your server, start the relay server binary:
//!
//! ``` bash
//! ./relay_v2 --port 4001 --secret-key-seed 0
//! ```
//!
//! Now let's make sure that the server is public, in other words let's make sure one can reach it
//! through the Internet. First, either manually replace `$RELAY_SERVER_IP` in the following
//! commands or `export RELAY_SERVER_IP=ipaddr` with the appropriate relay server `ipaddr` in
//! the dailing client and listening client.
//!
//! Now, from the dialing client:
//!
//! 1. Test that you can connect on Layer 3 (IP).
//!
//!    ``` bash
//!    ping $RELAY_SERVER_IP
//!    ```
//!
//! 2. Test that you can connect on Layer 4 (TCP).
//!
//!    ``` bash
//!    telnet $RELAY_SERVER_IP 4001
//!    ```
//!
//! 3. Test that you can connect via libp2p using [`libp2p-lookup`](https://github.com/mxinden/libp2p-lookup).
//!
//!    ``` bash
//!    ## For IPv4
//!    libp2p-lookup direct --address /ip4/$RELAY_SERVER_IP/tcp/4001
//!    ## For IPv6
//!    libp2p-lookup direct --address /ip6/$RELAY_SERVER_IP/tcp/4001
//!    ```
//!
//! The libp2p-lookup output should look something like:
//!
//!    ``` bash
//!    $ libp2p-lookup direct --address /ip4/111.11.111.111/tcp/4001
//!    Lookup for peer with id PeerId("12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN") succeeded.
//!
//!    Protocol version: "/TODO/0.0.1"
//!    Agent version: "rust-libp2p/0.36.0"
//!    Observed address: "/ip4/22.222.222.222/tcp/39212"
//!    Listen addresses:
//!            - "/ip4/127.0.0.1/tcp/4001"
//!            - "/ip4/111.11.111.111/tcp/4001"
//!            - "/ip4/10.48.0.5/tcp/4001"
//!            - "/ip4/10.124.0.2/tcp/4001"
//!    Protocols:
//!            - "/libp2p/circuit/relay/0.2.0/hop"
//!            - "/ipfs/ping/1.0.0"
//!            - "/ipfs/id/1.0.0"
//!            - "/ipfs/id/push/1.0.0"
//!    ```
//!
//! ## Setting up the listening client
//!
//! Either on the listening client machine directly, or on your local machine, compile the example
//! DCUtR client:
//!
//! ``` bash
//! ## Inside the rust-libp2p repository.
//! cargo build --example client -p libp2p-dcutr
//! ```
//!
//! You can find the binary at `target/debug/examples/client`. In case you built it locally, copy
//! it to your listening client machine.
//!
//! On the listening client machine:
//!
//! ``` bash
//! RUST_LOG=info ./client --secret-key-seed 1 --mode listen --relay-address /ip4/$RELAY_SERVER_IP/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN
//!
//! [2022-05-11T10:38:52Z INFO  client] Local peer id: PeerId("XXX")
//! [2022-05-11T10:38:52Z INFO  client] Listening on "/ip4/127.0.0.1/tcp/44703"
//! [2022-05-11T10:38:52Z INFO  client] Listening on "/ip4/XXX/tcp/44703"
//! [2022-05-11T10:38:54Z INFO  client] Relay told us our public address: "/ip4/XXX/tcp/53160"
//! [2022-05-11T10:38:54Z INFO  client] Told relay its public address.
//! [2022-05-11T10:38:54Z INFO  client] Relay accepted our reservation request.
//! [2022-05-11T10:38:54Z INFO  client] Listening on "/ip4/$RELAY_SERVER_IP/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN/p2p-circuit/p2p/XXX"
//! ```
//!
//! Now let's make sure that the listening client is not public, in other words let's make sure one
//! can not reach it directly through the Internet. From the dialing client test that you can not
//! connect on Layer 4 (TCP):
//!
//! ``` bash
//! telnet $LISTENING_CLIENT_IP_OBSERVED_BY_RELAY 53160
//! ```
//!
//! ## Connecting to the listening client from the dialing client
//!
//! ``` bash
//! RUST_LOG=info ./client --secret-key-seed 2 --mode dial --relay-address /ip4/$RELAY_SERVER_IP/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN --remote-peer-id 12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X
//! ```
//!
//! You should see the following logs appear:
//!
//! 1. The dialing client establishing a relayed connection to the listening client via the relay
//!    server. Note the [`/p2p-circuit` protocol](crate::multiaddr::Protocol::P2pCircuit) in the
//!    [`Multiaddr`](crate::Multiaddr).
//!
//!    ``` ignore
//!    [2022-01-30T12:54:10Z INFO  client] Established connection to PeerId("12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X") via Dialer { address: "/ip4/$RELAY_PEER_ID/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN/p2p-circuit/p2p/12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X", role_override: Dialer }
//!    ```
//!
//! 2. The listening client initiating a direct connection upgrade for the new relayed connection.
//!    Reported by [`dcutr`](crate::dcutr) through
//!    [`Event::RemoteInitiatedDirectConnectionUpgrade`](crate::dcutr::behaviour::Event::RemoteInitiatedDirectConnectionUpgrade).
//!
//!    ``` ignore
//!    [2022-01-30T12:54:11Z INFO  client] RemoteInitiatedDirectConnectionUpgrade { remote_peer_id: PeerId("12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X"), remote_relayed_addr: "/ip4/$RELAY_PEER_ID/tcp/4001/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN/p2p-circuit/p2p/12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X" }
//!    ```
//!
//! 3. The direct connection upgrade, also known as hole punch, succeeding. Reported by
//!    [`dcutr`](crate::dcutr) through
//!    [`Event::RemoteInitiatedDirectConnectionUpgrade`](crate::dcutr::behaviour::Event::DirectConnectionUpgradeSucceeded).
//!
//!    ``` ignore
//!    [2022-01-30T12:54:11Z INFO  client] DirectConnectionUpgradeSucceeded { remote_peer_id: PeerId("12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X") }
//!    ```
