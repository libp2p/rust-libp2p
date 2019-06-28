// Copyright 2019 Parity Technologies (UK) Ltd.
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

//! Management of tasks handling nodes.
//!
//! The core type is a [`task::Task`], which implements [`futures::Future`]
//! and connects and handles a node. A task receives and sends messages
//! ([`tasks::FromTaskMessage`], [`tasks::ToTaskMessage`]) to the outside.
//!
//! A set of tasks is managed by a [`Manager`] which creates tasks when a
//! node should be connected to (cf. [`Manager::add_reach_attempt`]) or
//! an existing connection to a node should be driven forward (cf.
//! [`Manager::add_connection`]). Tasks can be referred to by [`TaskId`]
//! and messages can be sent to individual tasks or all (cf.
//! [`Manager::start_broadcast`]). Messages produces by tasks can be
//! retrieved by polling the manager (cf. [`Manager::poll`]).

mod error;
mod manager;
mod task;

pub use error::Error;
pub use manager::{ClosedTask, TaskEntry, Manager, Event, StartTakeOver};

/// Task identifier.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TaskId(usize);

