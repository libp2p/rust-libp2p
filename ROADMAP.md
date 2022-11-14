# rust-libp2p Roadmap <!-- omit in toc -->

Below is a high level roadmap for the rust-libp2p project. Items are ordered by priority (high to
low).

This is a living document. Input is always welcome e.g. via GitHub issues or pull requests.

This is the roadmap of the Rust implementation of libp2p. See also the [general libp2p project
roadmap](https://github.com/libp2p/specs/blob/master/ROADMAP.md).

## Table of Contents <!-- omit in toc -->
- [🛣️ Milestones](#️-milestones)
  - [2022](#2022)
    - [Mid Q4 (November)](#mid-q4-november)
    - [Mid/End of Q4](#midend-of-q4)
  - [2023](#2023)
    - [Sometime Q1](#sometime-q1)
    - [End of Q1 (March)](#end-of-q1-march)
    - [Sometime Q2](#sometime-q2)
  - [Up Next](#up-next)
- [Appendix](#appendix)
  - [QUIC support](#quic-support)
  - [WebRTC support (browser-to-server)](#webrtc-support-browser-to-server)
  - [Cross Behaviour communication](#cross-behaviour-communication)
  - [Generic connection management](#generic-connection-management)
  - [Kademlia efficient querying](#kademlia-efficient-querying)
  - [Kademlia client mode](#kademlia-client-mode)
  - [Optimize Hole punching](#optimize-hole-punching)
  - [Streaming response protocol aka. the ultimate escape hatch](#streaming-response-protocol-aka-the-ultimate-escape-hatch)
  - [WebRTC support (browser-to-browser)](#webrtc-support-browser-to-browser)
  - [Improved Wasm support](#improved-wasm-support)
  - [Handshake optimizations](#handshake-optimizations)
  - [Bitswap implementation](#bitswap-implementation)
  - [WebTransport](#webtransport)

## 🛣️ Milestones

### 2022

#### Mid Q4 (November)
- [***➡️ test-plans/Interop tests for all existing/developing libp2p transports***](https://github.com/libp2p/test-plans/blob/master/ROADMAP.md#2-interop-test-plans-for-all-existingdeveloping-libp2p-transports)
- [***➡️ test-plans/Benchmarking using nix-builders***](https://github.com/libp2p/test-plans/blob/master/ROADMAP.md#1-benchmarking-using-nix-builders)

#### Mid/End of Q4
- [QUIC support](#quic-support)
- [WebRTC support (browser-to-server)](#webrtc-support-browser-to-server)

### 2023

#### Sometime Q1

#### End of Q1 (March)
- [***➡️ test-plans/Benchmarking using remote runners***](https://github.com/libp2p/test-plans/blob/master/ROADMAP.md#2-benchmarking-using-remote-runners)

#### Sometime Q2
- [WebRTC support (browser-to-browser](#webrtc-support-browser-to-browser)
- [Improved Wasm support](#improved-wasm-support)
- [Handshake optimizations](#handshake-optimizations)

### Up Next
- [***➡️ test-plans/Expansive protocol test coverage***](https://github.com/libp2p/test-plans/blob/master/ROADMAP.md#d-expansive-protocol-test-coverage)
- [Bitswap implementation](#bitswap-implementation)
- [WebTransport](#webtransport)

## Appendix

### QUIC support

| Category     | Status      | Target Completion | Tracking                                          | Dependencies                                                        | Dependents |
|--------------|-------------|-------------------|---------------------------------------------------|---------------------------------------------------------------------|------------|
| Connectivity | In progress | Q4/2022           | https://github.com/libp2p/rust-libp2p/issues/2883 | https://github.com/libp2p/test-plans/issues/53 |            |

QUIC has been on the roadmap for a long time. It enables various performance improvements as well as
higher hole punching success rates. We are close to finishing a first version with
https://github.com/libp2p/rust-libp2p/pull/2289 and will improve from there. See tracking issue
https://github.com/libp2p/rust-libp2p/issues/2883.

### WebRTC support (browser-to-server)

| Category     | Status      | Target Completion | Tracking                                 | Dependencies                                   | Dependents |
|--------------|-------------|-------------------|------------------------------------------|------------------------------------------------|------------|
| Connectivity | In progress | Q4/2022           | https://github.com/libp2p/specs/pull/412 | https://github.com/libp2p/test-plans/issues/53 |    [WebRTC (browser-to-browser)](#webrtc-support-browser-to-browser)        |


We are currently implementing WebRTC for **browser-to-server** connectivity in
https://github.com/libp2p/rust-libp2p/pull/2622. More specifically the server side. This will enable
browser nodes to connect to rust-libp2p nodes where the latter only have self-signed TLS
certificates. See https://github.com/libp2p/specs/pull/412 for in-depth motivation.

Long term we should enable rust-libp2p running in the browser via Wasm to use the browser's WebRTC
stack. Though that should only happen after improved Wasm support, see below.

### Cross Behaviour communication

| Category             | Status | Target Completion | Tracking                                          | Dependencies                                      | Dependents                                    |
|----------------------|--------|-------------------|---------------------------------------------------|---------------------------------------------------|-----------------------------------------------|
| Developer ergonomics | todo   | Q1/2023           | https://github.com/libp2p/rust-libp2p/issues/2680 | https://github.com/libp2p/rust-libp2p/issues/2832 | [Kademlia client mode](#kademlia-client-mode) |

Today `NetworkBehaviour` implementations like Kademlia, GossipSub or Circuit Relay v2 can not
communicate with each other, i.e. cannot make use of information known by another
`NetworkBehaviour` implementation. Users need to write the wiring code by hand to e.g. enable
Kademlia to learn protocols supported by a remote peer from Identify.

This roadmap item contains exchanging standard information about remote peers (e.g. supported
protocols) between `NetworkBehaviour` implementations.

Long term we might consider a generic approach for `NetworkBehaviours` to exchange data. Though that
would deserve its own roadmap item.

### Generic connection management

| Category             | Status | Target Completion | Tracking                                          | Dependencies | Dependents |
|----------------------|--------|-------------------|---------------------------------------------------|--------------|------------|
| Developer Ergonomics | todo   | Q1/2023           | https://github.com/libp2p/rust-libp2p/issues/2824 |              |            |

Today connection management functionality in rust-libp2p is limited. Building abstractions on top is
cumbersome and inefficient. See https://github.com/libp2p/rust-libp2p/issues/2824. Making connection
management generic allows users to build advanced and efficient abstractions on top of rust-libp2p

First draft is in https://github.com/libp2p/rust-libp2p/pull/2828

### Kademlia efficient querying

| Category     | Status      | Target Completion | Tracking                                        | Dependencies | Dependents |
|--------------|-------------|-------------------|-------------------------------------------------|--------------|------------|
| Optimization | in progress | Q1/2023           | https://github.com/libp2p/rust-libp2p/pull/2712 |              |            |

Users of rust-libp2p like [iroh](https://github.com/n0-computer/iroh) need this for low latency
usage of `libp2p-kad`. The rust-libp2p maintainers can pick this up unless iroh folks finish the
work before that.

### Kademlia client mode

| Category     | Status | Target Completion | Tracking                                          | Dependencies                                                    | Dependents |
|--------------|--------|-------------------|---------------------------------------------------|-----------------------------------------------------------------|------------|
| Optimization | todo   | Q1/2023           | https://github.com/libp2p/rust-libp2p/issues/2032 | [Cross behaviour communication](#cross-behaviour-communication) |            |

Kademlia client mode will enhance routing table health and thus have a positive impact on all
Kademlia operations.

### Optimize Hole punching

| Category     | Status | Target Completion | Tracking | Dependencies | Dependents |
|--------------|--------|-------------------|----------|--------------|------------|
| Optimization | todo   | Q1/2023           |          |              |            |

We released hole punching support with [rust-libp2p
`v0.43.0`](https://github.com/libp2p/rust-libp2p/releases/tag/v0.43.0), see also
https://github.com/libp2p/rust-libp2p/issues/2052. We are currently collecting data via the
[punchr](https://github.com/dennis-tra/punchr) project on the hole punching success rate. See also
[call for
action](https://discuss.libp2p.io/t/decentralized-nat-hole-punching-measurement-campaign/1616) in
case you want to help. Based on this data we will likely find many optimizations we can do to our
hole punching stack.

### Streaming response protocol aka. the ultimate escape hatch

| Category             | Status | Target Completion | Tracking                                          | Dependencies | Dependents |
|----------------------|--------|-------------------|---------------------------------------------------|--------------|------------|
| Developer ergonomics | todo   | Q1/2023           | https://github.com/libp2p/rust-libp2p/issues/2657 |              |            |

rust-libp2p is very opinionated on how to write peer-to-peer protocols. There are many good reasons
for this, and I think we should not change directions here. That said, the ultimate escape hatch -
allowing users to create a stream and do whatever they want with it - will make it easier for
newcomers to get started.

### WebRTC support (browser-to-browser)

| Category     | Status      | Target Completion | Tracking                                 | Dependencies                                   | Dependents |
|--------------|-------------|-------------------|------------------------------------------|------------------------------------------------|------------|
| Connectivity | todo        |     Q2/2023       | https://github.com/libp2p/specs/issues/475 | https://github.com/libp2p/rust-libp2p/pull/2622 https://github.com/libp2p/test-plans/issues/53 |            |


Once WebRTC for browser-to-server is complete, we can begin work on **browser-to-browser** and complete the WebRTC connectivity story.
The specification needs to be written and completed first.

### Improved Wasm support

| Category             | Status | Target Completion | Tracking                                          | Dependencies | Dependents                                 |
|----------------------|--------|-------------------|---------------------------------------------------|--------------|--------------------------------------------|
| Developer ergonomics | todo   | Q2/2023           | https://github.com/libp2p/rust-libp2p/issues/2617 |              | WebRTC browser-to-server and browser side |

The project supports Wasm already today, though the developer experience is cumbersome at best.
Properly supporting Wasm opens rust-libp2p to hole new set of use-cases. I would love for this to
happen earlier. Though (a) I think we should prioritize improving existing functionality over new
functionality and (b) we don't have high demand for this feature from the community. (One could
argue that that demand follows this roadmap item and not the other way round.)

### Handshake optimizations

| Category     | Status | Target Completion | Tracking                                                                                                                                                | Dependencies | Dependents |
|--------------|--------|-------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------|--------------|------------|
| Optimization | todo   | Q2/2023           | Security protocol in multiaddr https://github.com/libp2p/specs/pull/353 and early muxer negotiation https://github.com/libp2p/rust-libp2p/issues/2994 |              |            |

Short term, investing into rust-libp2p's QUIC support will likely give us a larger performance win,
thus neither of the two optimizations is planned for 2022. While great to have, it has not been
requested from any rust-libp2p users.

Long term, given that this will give us a great performance gain, we should definitely tackle it. It
also allows us to catch up and thus be consistent with go-libp2p.

### Bitswap implementation

| Category | Status | Target Completion | Tracking                                          | Dependencies | Dependents |
|----------|--------|-------------------|---------------------------------------------------|--------------|------------|
|          | todo   |                   | https://github.com/libp2p/rust-libp2p/issues/2632 |              |            |

I think this is a common component that many users need to build peer-to-peer applications. In
addition, it is very performance critical and thus likely challenges many of our existing designs in
rust-libp2p.

I would prioritize it below [handshake optimization](#handshake-optimizations) following the
convention of improving existing components over introducing new ones. Users have and can implement
their own implementations and are thus not blocked on the rust-libp2p project.

### WebTransport

| Category                    | Status | Target Completion | Tracking                                          | Dependencies                       | Dependents |
|-----------------------------|--------|-------------------|---------------------------------------------------|------------------------------------|------------|
| Connectivity / optimization | todo   |                   | https://github.com/libp2p/rust-libp2p/issues/2993 | [QUIC](#experimental-quic-support) |            |

A WebTransport implementation in rust-libp2p will enable browsers to connect to rust-libp2p nodes
where the latter only have a self-signed TLS certificate. Compared to WebRTC, this would likely be
more performant. It is dependent on QUIC support in rust-libp2p. Given that we will support WebRTC
(browser-to-server) this is not a high priority.
