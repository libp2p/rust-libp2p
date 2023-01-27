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

/// Implementation of [`NetworkBehaviour`] that combines two underlying implementations.
#[cfg(feature = "macros")]
#[derive(Debug, Default, libp2p_swarm_derive::NetworkBehaviour)]
#[behaviour(prelude = "crate::derive_prelude")]
pub struct And<A, B> {
    first: A,
    second: B,
}

#[cfg(feature = "macros")]
impl<A, B> And<A, B>
where
    A: crate::NetworkBehaviour,
    B: crate::NetworkBehaviour,
{
    /// Create a new `And` from two [`NetworkBehaviour`]s.
    pub fn new(first: A, second: B) -> And<A, B> {
        Self { first, second }
    }

    /// Returns a reference to the first `NetworkBehaviour`.
    pub fn first_as_ref(&self) -> &A {
        &self.first
    }

    /// Returns a mutable reference to the first underlying `NetworkBehaviour`.
    pub fn first_as_mut(&mut self) -> &mut A {
        &mut self.first
    }

    /// Returns a reference to the second `NetworkBehaviour`.
    pub fn second_as_ref(&self) -> &B {
        &self.second
    }

    /// Returns a mutable reference to the second underlying `NetworkBehaviour`.
    pub fn second_as_mut(&mut self) -> &mut B {
        &mut self.second
    }
}
