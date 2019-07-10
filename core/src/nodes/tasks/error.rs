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

use crate::nodes::handled_node::HandledNodeError;
use std::{fmt, error};

/// Error that can happen in a task.
#[derive(Debug)]
pub enum Error<R, H> {
    /// An error happend while we were trying to reach the node.
    Reach(R),
    /// An error happened after the node has been reached.
    Node(HandledNodeError<H>)
}

impl<R, H> fmt::Display for Error<R, H>
where
    R: fmt::Display,
    H: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Reach(err) => write!(f, "reach error: {}", err),
            Error::Node(err) => write!(f, "node error: {}", err)
        }
    }
}

impl<R, H> error::Error for Error<R, H>
where
    R: error::Error + 'static,
    H: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Reach(err) => Some(err),
            Error::Node(err) => Some(err)
        }
    }
}

