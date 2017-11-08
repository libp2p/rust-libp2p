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
