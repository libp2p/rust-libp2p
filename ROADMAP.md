# rust-libp2p Roadmap

Below is a high level roadmap for the rust-libp2p project. Items are ordered by priority (high to
low).

This is a living document. Input is always welcome e.g. via GitHub issues or pull requests.

This is the roadmap of the Rust implementation of libp2p. See also the [general libp2p project
roadmap](https://github.com/libp2p/specs/blob/master/ROADMAP.md).

## Cross Behaviour communication

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

## Kademlia client mode

| Category     | Status | Target Completion | Tracking                                          | Dependencies                                                    | Dependents |
|--------------|--------|-------------------|---------------------------------------------------|-----------------------------------------------------------------|------------|
| Optimization | todo   | Q1/2023           | https://github.com/libp2p/rust-libp2p/issues/2032 | [Cross behaviour communication](#cross-behaviour-communication) |            |

Kademlia client mode will enhance routing table health and thus have a positive impact on all
Kademlia operations.

## QUIC - evaluate and move to quinn

| Category     | Status | Target Completion | Tracking                                          | Dependencies | Dependents |
|--------------|--------|-------------------|---------------------------------------------------|--------------|------------|
| Connectivity | todo   | Q2/2023           | https://github.com/libp2p/rust-libp2p/issues/2883 |              |            |

We added alpha support for QUIC in Q4/2022 wrapping `quinn-proto`. Evaluate using `quinn` directly, replacing the wrapper.

## Optimize Hole punching

| Category     | Status | Target Completion | Tracking | Dependencies | Dependents |
|--------------|--------|-------------------|----------|--------------|------------|
| Optimization | todo   | Q2/2023           |          |              |            |

We released hole punching support with [rust-libp2p
`v0.43.0`](https://github.com/libp2p/rust-libp2p/releases/tag/v0.43.0), see also
https://github.com/libp2p/rust-libp2p/issues/2052. We are currently collecting data via the
[punchr](https://github.com/dennis-tra/punchr) project on the hole punching success rate. See also
[call for
action](https://discuss.libp2p.io/t/decentralized-nat-hole-punching-measurement-campaign/1616) in
case you want to help. Based on this data we will likely find many optimizations we can do to our
hole punching stack.

## Improved Wasm support

| Category             | Status | Target Completion | Tracking                                          | Dependencies | Dependents                                   |
|----------------------|--------|-------------------|---------------------------------------------------|--------------|----------------------------------------------|
| Developer ergonomics | todo   | Q3/2023           | https://github.com/libp2p/rust-libp2p/issues/2617 |              | [WebRTC](#webrtc-support-browser-to-browser) |

The project supports Wasm already today, though the developer experience is cumbersome at best.
Properly supporting Wasm opens rust-libp2p to a whole new set of use-cases. I would love for this to
happen earlier. Though (a) I think we should prioritize improving existing functionality over new
functionality and (b) we don't have high demand for this feature from the community. (One could
argue that that demand follows this roadmap item and not the other way round.)

## WebRTC support (browser-to-browser)

| Category     | Status | Target Completion | Tracking                                   | Dependencies                                                                              | Dependents |
|--------------|--------|-------------------|--------------------------------------------|-------------------------------------------------------------------------------------------|------------|
| Connectivity | todo   | Q3/2023           | https://github.com/libp2p/specs/issues/475 | [Improved WASM support](#improved-wasm-support), https://github.com/libp2p/specs/pull/497 |            |


Once WebRTC for browser-to-server is complete, we can begin work on **browser-to-browser** and complete the WebRTC connectivity story.
We need to improve rust-libp2p's WASM story first.

## WebTransport

| Category                    | Status | Target Completion | Tracking                                          | Dependencies                       | Dependents |
|-----------------------------|--------|-------------------|---------------------------------------------------|------------------------------------|------------|
| Connectivity / optimization | todo   |                   | https://github.com/libp2p/rust-libp2p/issues/2993 | [QUIC](#experimental-quic-support) |            |

A WebTransport implementation in rust-libp2p will enable browsers to connect to rust-libp2p nodes
where the latter only have a self-signed TLS certificate. Compared to WebRTC, this would likely be
more performant. It is dependent on QUIC support in rust-libp2p. Given that we will support WebRTC
(browser-to-server) this is not a high priority.

## Done

### Alpha QUIC support

| Category     | Status | Target Completion | Tracking                                          | Dependencies                                   | Dependents |
|--------------|--------|-------------------|---------------------------------------------------|------------------------------------------------|------------|
| Connectivity | Done   | Q4/2022           | https://github.com/libp2p/rust-libp2p/issues/2883 | https://github.com/libp2p/test-plans/issues/53 |            |

QUIC has been on the roadmap for a long time. It enables various performance improvements as well as
higher hole punching success rates. We are close to finishing a first version with
https://github.com/libp2p/rust-libp2p/pull/2289.

### WebRTC support (browser-to-server)

| Category     | Status | Target Completion | Tracking                                 | Dependencies                                   | Dependents                                                        |
|--------------|--------|-------------------|------------------------------------------|------------------------------------------------|-------------------------------------------------------------------|
| Connectivity | Done   | Q4/2022           | https://github.com/libp2p/specs/pull/412 | https://github.com/libp2p/test-plans/pull/100 | [WebRTC (browser-to-browser)](#webrtc-support-browser-to-browser) |


We are currently implementing WebRTC for **browser-to-server** connectivity in
https://github.com/libp2p/rust-libp2p/pull/2622. More specifically the server side. This will enable
browser nodes to connect to rust-libp2p nodes where the latter only have self-signed TLS
certificates. See https://github.com/libp2p/specs/pull/412 for in-depth motivation.

Long term we should enable rust-libp2p running in the browser via Wasm to use the browser's WebRTC
stack. Though that should only happen after improved Wasm support, see below.

### Kademlia efficient querying

| Category     | Status      | Target Completion | Tracking                                        | Dependencies | Dependents |
|--------------|-------------|-------------------|-------------------------------------------------|--------------|------------|
| Optimization | done        | Q1/2023           | https://github.com/libp2p/rust-libp2p/pull/2712 |              |            |

Users of rust-libp2p like [iroh](https://github.com/n0-computer/iroh) need this for low latency
usage of `libp2p-kad`. The rust-libp2p maintainers can pick this up unless iroh folks finish the
work before that.

### Generic connection management

| Category             | Status      | Target Completion | Tracking                                          | Dependencies | Dependents |
|----------------------|-------------|-------------------|---------------------------------------------------|--------------|------------|
| Developer Ergonomics | done | Q1/2023           | https://github.com/libp2p/rust-libp2p/issues/2824 |              |            |

Today connection management functionality in rust-libp2p is limited. Building abstractions on top is
cumbersome and inefficient. See https://github.com/libp2p/rust-libp2p/issues/2824. Making connection
management generic allows users to build advanced and efficient abstractions on top of rust-libp2p.
