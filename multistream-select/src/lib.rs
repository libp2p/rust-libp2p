// Copyright 2017 Parity Technologies (UK) Ltd.

// Libp2p-rs is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Libp2p-rs is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Libp2p-rs.  If not, see <http://www.gnu.org/licenses/>.

//! # Multistream-select
//!
//! Multistream-select is the "main" protocol of libp2p.
//! Whenever a connection opens between two peers, it starts talking in `multistream-select`.
//!
//! The purpose of `multistream-select` is to choose which protocol we are going to use. As soon as
//! both sides agree on a given protocol, the socket immediately starts using it and multistream is
//! no longer relevant.
//!
//! However note that `multistream-select` is also sometimes used on top of another protocol such
//! as secio or multiplex. For example, two hosts can use `multistream-select` to decide to use
//! secio, then use `multistream-select` again (wrapped inside `secio`) to decide to use
//! `multiplex`, then use `multistream-select` one more time (wrapped inside `secio` and
//! `multiplex`) to decide to use the final actual protocol.

extern crate bytes;
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;

mod dialer_select;
mod error;
mod listener_select;
mod tests;

pub mod protocol;

pub use self::dialer_select::dialer_select_proto;
pub use self::error::ProtocolChoiceError;
pub use self::listener_select::listener_select_proto;
