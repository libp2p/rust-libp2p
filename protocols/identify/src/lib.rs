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

//! Implementation of the [Identify](https://github.com/libp2p/specs/blob/master/identify/README.md) protocol.
//!
//! This implementation of the protocol periodically exchanges
//! [`Info`] messages between the peers on an established connection.
//!
//! At least one identification request is sent on a newly established
//! connection, beyond which the behaviour does not keep connections alive.
//!
//! # Important Discrepancies
//!
//! - **Using Identify with other protocols** Unlike some other libp2p implementations,
//! rust-libp2p does not treat Identify as a core protocol. This means that other protocols cannot
//! rely upon the existence of Identify, and need to be manually hooked up to Identify in order to
//! make use of its capabilities.
//!
//! # Usage
//!
//! The [`Behaviour`] struct implements a [`NetworkBehaviour`](libp2p_swarm::NetworkBehaviour)
//! that negotiates and executes the protocol on every established connection, emitting
//! [`Event`]s.

pub use self::behaviour::{Behaviour, Config, Event};
pub use self::protocol::{Info, UpgradeError, PROTOCOL_NAME, PUSH_PROTOCOL_NAME};

#[deprecated(
    since = "0.40.0",
    note = "Use re-exports that omit `Identify` prefix, i.e. `libp2p::identify::Config`"
)]
pub type IdentifyConfig = Config;

#[deprecated(
    since = "0.40.0",
    note = "Use re-exports that omit `Identify` prefix, i.e. `libp2p::identify::Event`"
)]
pub type IdentifyEvent = Event;

#[deprecated(since = "0.40.0", note = "Use libp2p::identify::Behaviour instead.")]
pub type Identify = Behaviour;

#[deprecated(
    since = "0.40.0",
    note = "Use re-exports that omit `Identify` prefix, i.e. `libp2p::identify::Info`"
)]
pub type IdentifyInfo = Info;

mod behaviour;
mod handler;
mod protocol;

#[allow(clippy::derive_partial_eq_without_eq)]
mod structs_proto {
    include!(concat!(env!("OUT_DIR"), "/structs.rs"));
}
