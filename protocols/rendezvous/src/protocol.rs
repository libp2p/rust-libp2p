// Copyright 2021 COMIT Network.
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

use crate::codec::RendezvousCodec;
use asynchronous_codec::Framed;
use libp2p_core::upgrade::FromFnUpgrade;
use libp2p_core::{upgrade, Endpoint};
use libp2p_swarm::NegotiatedSubstream;
use std::future;
use upgrade::from_fn;
use void::Void;

pub fn new() -> Rendezvous {
    from_fn(
        b"/rendezvous/1.0.0",
        Box::new(|socket, _| future::ready(Ok(Framed::new(socket, RendezvousCodec::default())))),
    )
}

pub type Rendezvous = FromFnUpgrade<
    &'static [u8],
    Box<
        dyn Fn(
                NegotiatedSubstream,
                Endpoint,
            )
                -> future::Ready<Result<Framed<NegotiatedSubstream, RendezvousCodec>, Void>>
            + Send
            + 'static,
    >,
>;
