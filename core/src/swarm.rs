// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! High level manager of the network.
//!
//! A [`Swarm`] contains the state of the network as a whole. The entire behaviour of a
//! libp2p network can be controlled through the `Swarm`.
//!
//! # Initializing a Swarm
//!
//! Creating a `Swarm` requires three things:
//!
//!  1. A network identity of the local node in form of a [`PeerId`].
//!  2. An implementation of the [`Transport`] trait. This is the type that will be used in
//!     order to reach nodes on the network based on their address. See the [`transport`] module
//!     for more information.
//!  3. An implementation of the [`NetworkBehaviour`] trait. This is a state machine that
//!     defines how the swarm should behave once it is connected to a node.
//!
//! # Network Behaviour
//!
//! The `NetworkBehaviour` trait is implemented on types that indicate to the swarm how it should
//! behave. This includes which protocols are supported and which nodes to try to connect to.
//!

mod behaviour;
mod swarm;
mod registry;

pub mod toggle;

pub use crate::nodes::raw_swarm::ConnectedPoint;
pub use self::behaviour::{NetworkBehaviour, NetworkBehaviourAction, NetworkBehaviourEventProcess, PollParameters};
pub use self::swarm::{SwarmPollParameters, ExpandedSwarm, Swarm, SwarmBuilder};
