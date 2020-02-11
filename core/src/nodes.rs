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

//! Low-level networking primitives.
//!
//! Contains structs that are aiming at providing very precise control over what happens over the
//! network.
//!
//! The more complete and highest-level struct is the `Network`. The `Network` directly or
//! indirectly uses all the other structs of this module.

pub mod collection;
pub mod handled_node;
pub mod tasks;
pub mod listeners;
pub mod node;
pub mod network;

pub use collection::ConnectionInfo;
pub use node::Substream;
pub use handled_node::{NodeHandlerEvent, NodeHandlerEndpoint};
pub use network::{Peer, Network, NetworkEvent};
pub use listeners::ListenerId;

