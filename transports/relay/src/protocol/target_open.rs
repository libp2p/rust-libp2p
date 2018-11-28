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

use bytes::Bytes;
use codec::Codec;
use copy;
use libp2p_core::{upgrade, Endpoint, Multiaddr, PeerId};
use futures::{future::{self, Either::{A, B}, FutureResult}, prelude::*};
use message::{CircuitRelay, CircuitRelay_Peer, CircuitRelay_Status, CircuitRelay_Type};
use std::{io, iter};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};
use utility::{Peer, status};      // TODO: move these here
use void::Void;

/// Upgrade that negotiates the relay protocol and returns the socket.
#[derive(Debug, Clone)]
pub struct TargetOpen {
    /// The message to send to the destination. Pre-computed.
    message: CircuitRelay,
}

impl TargetOpen {
    /// Creates a `TargetOpen`. Must pass the parameters of the message.
    pub(crate) fn new(from: Peer, dest: Peer) -> Self {
        let mut msg = CircuitRelay::new();
        msg.set_field_type(CircuitRelay_Type::STOP);

        let mut f = CircuitRelay_Peer::new();
        f.set_id(from.id.as_bytes().to_vec());
        for a in &from.addrs {
            f.mut_addrs().push(a.to_bytes())
        }
        msg.set_srcPeer(f);

        let mut d = CircuitRelay_Peer::new();
        d.set_id(dest.id.as_bytes().to_vec());
        for a in &dest.addrs {
            d.mut_addrs().push(a.to_bytes())
        }
        msg.set_dstPeer(d);

        TargetOpen {
            message: msg,
        }
    }
}

impl upgrade::UpgradeInfo for TargetOpen {
    type UpgradeId = ();
    type NamesIter = iter::Once<(Bytes, Self::UpgradeId)>;

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/libp2p/relay/circuit/0.1.0"), ()))
    }
}

impl<TSubstream> upgrade::OutboundUpgrade<TSubstream> for TargetOpen
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type Output = TSubstream;
    type Error = Void;
    type Future = FutureResult<Self::Output, Self::Error>;

    #[inline]
    fn upgrade_outbound(self, conn: TSubstream, _: ()) -> Self::Future {
        future::ok(conn)
    }
}
