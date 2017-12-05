// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! # Peerstore
//!
//! The `peerstore` crate allows one to store information about a peer.
//! It is similar to a key-value database, where the keys are multihashes (generally the hash of
//! the public key of the peer, but that is not enforced by this crate) and the values are the
//! public key and a list of multiaddresses with a time-to-live.
//!
//! This crate consists in a generic `Peerstore` trait and various backends.
//!
//! Note that the peerstore implementations do not consider information inside a peer store to be
//! critical. In case of an error (eg. corrupted file, disk error, etc.) they will prefer to lose
//! data rather than returning the error.

extern crate base58;
extern crate datastore;
extern crate futures;
extern crate multiaddr;
extern crate owning_ref;
extern crate serde;
#[macro_use]
extern crate serde_derive;

pub use self::peerstore::{Peerstore, PeerAccess};

#[macro_use]
mod peerstore_tests;

pub mod json_peerstore;
pub mod memory_peerstore;
mod peerstore;
mod peer_info;

pub type PeerId = Vec<u8>;
pub type TTL = std::time::Duration;
