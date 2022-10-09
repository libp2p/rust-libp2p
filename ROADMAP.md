# rust-libp2p Roadmap

<!-- markdown-toc start - Don't edit this section. Run M-x markdown-toc-refresh-toc -->
**Table of Contents**

- [rust-libp2p Roadmap](#rust-libp2p-roadmap)
    - [Testground abstraction](#testground-abstraction)
    - [TLS support](#tls-support)
    - [QUIC support](#quic-support)
    - [WebRTC support (browser-to-server)](#webrtc-support-browser-to-server)
    - [Refactor event handling in `Swarm`](#refactor-event-handling-in-swarm)
    - [Cross Behaviour communication](#cross-behaviour-communication)
    - [Decouple ConnectionHandler from {In,Out}boundUpgrade](#decouple-connectionhandler-from-inoutboundupgrade)
    - [Generic connection management](#generic-connection-management)
    - [Kademlia efficient querying](#kademlia-efficient-querying)
    - [Kademlia client mode](#kademlia-client-mode)
    - [Optimize Hole punching](#optimize-hole-punching)
    - [Streaming response protocol aka. the ultimate escape hatch](#streaming-response-protocol-aka-the-ultimate-escape-hatch)
    - [Improved WASM support](#improved-wasm-support)
    - [Handshake optimizations](#handshake-optimizations)
    - [Bitswap implementation](#bitswap-implementation)
    - [WebTransport](#webtransport)

<!-- markdown-toc end -->

## Testground abstraction

Status: todo

Target completion: Q4/2022

Tracking:

Past discussions:

- https://github.com/libp2p/test-plans/pull/49#issuecomment-1267175415

Dependencies:

Dependents:

- [QUIC](#quic-support)
- [WebRTC](#webrtc-support-browser-to-server)

We now have the infrastructure to run cross-implementation and cross-version compatilibilty tests
via https://github.com/libp2p/test-plans and Testground. This setup is rather bare-bone, i.e. today
it only runs a single TCP test. Long term we need to come up with the abstractions to test many
dimensions, e.g. implementations (Go, JS, Rust, Nim), version, transport, ... .

This will enable us to test QUIC and WebRTC against rust-libp2p itself as well as js-libp2p and
go-libp2p.

## TLS support

Status: In progress

Target completion: Q4/2022

Tracking: https://github.com/libp2p/rust-libp2p/pull/2945

Dependencies:

Dependents:
- [QUIC](#quic-support)

This allows us to secure both TCP and QUIC connections using TLS. This is a requirement for QUIC
support. Running TLS on top of TCP is a nice to have, since we already have noise.

## QUIC support

Status: In progress

Target completion: Q4/2022

Tracking: https://github.com/libp2p/rust-libp2p/issues/2883

Dependencies:
- [TLS](#tls-support)
- [Testground](#testground)

Dependents:

QUIC has been on the roadmap for a long time. It enables various performance improvements as well as
higher hole punching success rates. We are close to finishing a first version with
https://github.com/libp2p/rust-libp2p/pull/2289. Long term there is lots more to do, see tracking
issue https://github.com/libp2p/rust-libp2p/issues/2883.

## WebRTC support (browser-to-server)

Status: In progress

Target completion: Q4/2022

Tracking: https://github.com/libp2p/specs/pull/412

Dependencies:
- [TLS](#tls-support)
- [Testground](#testground)

Dependents:

We are currently implementing WebRTC for **browser-to-server** connectivity in
https://github.com/libp2p/rust-libp2p/pull/2622. More specifically the server side. This will enable
browser nodes to connect to rust-libp2p nodes where the latter only have self-signed TLS
certificates. See https://github.com/libp2p/specs/pull/412 for in-depth motivation.

Long term we should enable rust-libp2p running in the browser via WASM to use the browser's WebRTC
stack. Though that should only happen after improved WASM support, see below.

## Refactor event handling in `Swarm`

Status: In progress

Target completion: Q4/2022

Tracking: https://github.com/libp2p/rust-libp2p/issues/2832

Dependencies:

Dependents:

- [Cross behaviour communication](#cross-behaviour-communication)

More specifically replace the `inject_*` methods on `NetworkBehaviour` and `ConnectionHandler` with
consolidated `on_*_event` handlers. See https://github.com/libp2p/rust-libp2p/issues/2832 and
https://github.com/libp2p/rust-libp2p/pull/2867 for details.

While a rather small change, this will make using rust-libp2p easier. In my eyes this is a
requirement for generic connection management and cross behaviour communication as either would
otherwise introduce too much complexity.

## Cross Behaviour communication

Status: todo

Target completion: Q1/2023

Tracking: https://github.com/libp2p/rust-libp2p/issues/2680

Dependencies:

- [Refactor event handling](#refactor-event-handling-in-swarm)

Dependents:

- [Kademlia client mode](#kademlia-client-mode)

## Decouple ConnectionHandler from {In,Out}boundUpgrade

Status: todo

Target completion: Q1/2023

Tracking: https://github.com/libp2p/rust-libp2p/issues/2863

Dependencies:

Dependents:

I think this will simplify existing implementations and lower the learning curve required to get
started with rust-libp2p.

## Generic connection management

Target completion: Q1/2023

See https://github.com/libp2p/rust-libp2p/issues/2824 for motivation. Given that this will enable
downstream users to easier integrate with rust-libp2p, I think this counts as a "improving existing
components" over "introducing a new component".

First draft is in https://github.com/libp2p/rust-libp2p/pull/2828

## Kademlia efficient querying

Status: in progress

Target completion: Q1/2023

Tracking: https://github.com/libp2p/rust-libp2p/pull/2712

Dependencies:

Dependents:

Users of rust-libp2p like [iroh](https://github.com/n0-computer/iroh) need this for low latency
usage of `libp2p-kad`. The rust-libp2p maintainers can pick this up unless iroh folks finish the
work before that.

## Kademlia client mode

Status: todo

Target completion: Q1/2023

Tracking: https://github.com/libp2p/rust-libp2p/issues/2032

Dependencies:

- [Cross behaviour communication](#cross-behaviour-communication)

Dependents:

## Optimize Hole punching

Status: todo

Target completion: Q1/2023

Dependencies:

Dependents:

We released hole punching support with [rust-libp2p
`v0.43.0`](https://github.com/libp2p/rust-libp2p/releases/tag/v0.43.0), see also
https://github.com/libp2p/rust-libp2p/issues/2052. We are currently collecting data via the
[punchr](https://github.com/dennis-tra/punchr) project on the hole punching success rate. See also
[call for
action](https://discuss.libp2p.io/t/decentralized-nat-hole-punching-measurement-campaign/1616) in
case you want to help. Based on this data we will likely find many optimizations we can do to our
hole punching stack.

## Streaming response protocol aka. the ultimate escape hatch

Status: todo

Target completion: Q1/2023

Tracking: https://github.com/libp2p/rust-libp2p/issues/2657

rust-libp2p is very opinionated on how to write peer-to-peer protocols. There are many good reasons
for this, and I think we should not change directions here. That said, the ultimate escape hatch -
allowing users to create a stream and do whatever they want with it - will make it easier for
newcomers to get started.

## Improved WASM support

Status: todo

Target completion: Q2/2023

Tracking: https://github.com/libp2p/rust-libp2p/issues/2617

Dependencies:

Dependents:

- WebRTC browser-to-browser and browser side

This opens rust-libp2p to hole new set of use-cases. I would love for this to happen earlier. Though
(a) I think we should prioritize improving existing functionality over new functionality and (b) we
don't have high demand for this feature from the community. (One could argue that that demand
follows this roadmap item and not the other way round.)

## Handshake optimizations

Status: todo

Target completion: Q2/2023

Tracking:

- Security protocol in multiaddr https://github.com/libp2p/specs/pull/353
- Early muxer negotiation https://github.com/libp2p/rust-libp2p/issues/2994

Short term, investing into rust-libp2p's QUIC support will likely give us a larger performance win,
thus neither of the two optimizations is planned for 2022. While great to have, it has not been
requested from any rust-libp2p users.

Long term, given that this will give us a great performance gain, we should definitely tackle it. It
also allows us to catch up and thus be consistent with go-libp2p.

## Bitswap implementation

Status: todo

Target completion: unknown

Tracking: https://github.com/libp2p/rust-libp2p/issues/2632

I think this is a common component that many users need to build peer-to-peer applications. In
addition, it is very performance critical and thus likely challenges many of our existing designs in
rust-libp2p.

I would prioritize it below [Muxer handshake optimization](#muxer-handshake-optimization) following
the convention of improving existing components over introducing new ones. Users have and can
implement their own implementations and are thus not blocked on the rust-libp2p project.

## WebTransport

Status: todo

Target completion: unknown

Tracking: https://github.com/libp2p/rust-libp2p/issues/2993

Dependencies:

- [QUIC](#quic-support)

Dependents:

A WebTransport implementation in rust-libp2p will enable browsers to connect to rust-libp2p nodes
where the latter only have a self-signed TLS certificate. Compared to WebRTC, this would likely be
more performant. It is dependent on QUIC support in rust-libp2p. Given that we will support WebRTC
(browser-to-server) this is not a high priority.
