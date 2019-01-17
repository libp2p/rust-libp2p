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
use libp2p_core::{upgrade, Multiaddr, PeerId};
use futures::prelude::*;
use message::{CircuitRelay, CircuitRelay_Peer, CircuitRelay_Type};
use std::{error, io, iter};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};

/// Request to act as a relay to a third party.
///
/// If we take a destination where a *source* wants to talk to a *destination* through a *relay*,
/// this struct is the message that the *source* sends to the *relay* at initialization. The
/// parameters passed to `RelayProxyRequest::new()` are the information of the *destination*.
///
/// The upgrade should be performed on a substream to the *relay*.
///
/// If the upgrade succeeds, the substream is returned and is now a brand new connection pointing
/// to the *destination*.
// TODO: debug
pub struct RelayProxyRequest {
    /// Message that we will send to the relay. Prepared in advance.
    message: CircuitRelay,
}

impl RelayProxyRequest {
    /// Builds a request for the target to act as a relay to a third party.
    pub fn new(dest_id: PeerId, dest_addresses: impl IntoIterator<Item = Multiaddr>) -> Self {
        let mut msg = CircuitRelay::new();
        msg.set_field_type(CircuitRelay_Type::HOP);

        let mut dest = CircuitRelay_Peer::new();
        dest.set_id(dest_id.as_bytes().to_vec());
        for a in dest_addresses {
            dest.mut_addrs().push(a.to_bytes())
        }
        msg.set_dstPeer(dest);

        RelayProxyRequest {
            message: msg,
        }
    }
}

impl upgrade::UpgradeInfo for RelayProxyRequest {
    type UpgradeId = ();
    type NamesIter = iter::Once<(Bytes, Self::UpgradeId)>;

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/libp2p/relay/circuit/0.1.0"), ()))
    }
}

impl<TSubstream> upgrade::OutboundUpgrade<TSubstream> for RelayProxyRequest
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type Output = TSubstream;
    type Error = Box<error::Error>;
    type Future = RelayProxyRequestFuture<TSubstream>;

    #[inline]
    fn upgrade_outbound(self, conn: TSubstream, _: ()) -> Self::Future {
        let framed = Framed::new(conn, Codec::new());
        RelayProxyRequestFuture {
            stream: framed,
            message: Some(self.message),
            flushed: false,
        }
    }
}

/// Future that drives upgrading the `RelayProxyRequest`.
// TODO: Debug
#[must_use = "futures do nothing unless polled"]
pub struct RelayProxyRequestFuture<TSubstream> {
    /// The stream to the relay.
    stream: Framed<TSubstream, Codec>,

    /// Message to send to the relay, or `None` if it has been successfully sent.
    message: Option<CircuitRelay>,

    /// If true, we successfully flushed after sending.
    flushed: bool,
}

impl<TSubstream> Future for RelayProxyRequestFuture<TSubstream>
where TSubstream: AsyncRead + AsyncWrite,
{
    type Item = TSubstream;
    type Error = Box<error::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(message) = self.message.take() {
            match self.stream.start_send(message)? {
                AsyncSink::Ready => (),
                AsyncSink::NotReady(message) => {
                    self.message = Some(message);
                    return Ok(Async::NotReady);
                },
            }
        }

        if !self.flushed {
            try_ready!(self.stream.poll_complete());
            self.flushed = true;
        }

        panic!()        // FIXME:
        /*match try_ready!(self.stream.poll()) {
            Some(message) => {

            },
            None => {

            },
        }*/
    }
}
