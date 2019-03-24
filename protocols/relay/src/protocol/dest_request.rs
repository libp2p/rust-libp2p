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

use crate::message::{CircuitRelay, CircuitRelay_Status};
use crate::protocol::Peer;
use bytes::Buf as _;
use futures::{prelude::*, try_ready};
use libp2p_core::{upgrade, Multiaddr, PeerId};
use protobuf::Message as _;
use std::{error, io};
use tokio_io::{AsyncRead, AsyncWrite};

/// Request from a remote for us to become a destination.
///
/// If we take a situation where a *source* wants to talk to a *destination* through a *relay*, and
/// we are the *destination*, this struct is a message that the *relay* sent to us. The
/// parameters passed to `RelayDestinationRequest::new()` are the information of the *source*.
///
/// If the upgrade succeeds, the substream is returned and we will receive data sent from the
/// source on it.
// TODO: debug
#[must_use = "A destination request should be either accepted or denied"]
pub struct RelayDestinationRequest<TSubstream> {
    /// The stream to the source.
    stream: upgrade::Negotiated<TSubstream>,
    /// Source of the request.
    from: Peer,
}

impl<TSubstream> RelayDestinationRequest<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    /// Creates a `RelayDestinationRequest`.
    pub(crate) fn new(stream: upgrade::Negotiated<TSubstream>, from: Peer) -> Self {
        RelayDestinationRequest { stream, from }
    }

    /// Returns the peer id of the source that is being relayed.
    pub fn source_id(&self) -> &PeerId {
        &self.from.peer_id
    }

    /// Returns the addresses of the source that is being relayed.
    pub fn source_addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.from.addrs.iter()
    }

    /// Accepts the request.
    ///
    /// The returned `Future` sends back a success message then returns the raw stream. This raw
    /// stream then points to the source (as retreived with `source_id()` and `source_addresses()`).
    pub fn accept(self) -> RelayDestinationAcceptFuture<TSubstream> {
        let mut msg = CircuitRelay::new();
        msg.set_code(CircuitRelay_Status::SUCCESS);
        let msg_bytes = msg
            .write_to_bytes()
            .expect("all the mandatory fields are always filled; QED");
        RelayDestinationAcceptFuture {
            inner: Some(self.stream),
            message: io::Cursor::new(msg_bytes),
        }
    }

    /// Refuses the request.
    ///
    /// The returned `Future` gracefully shuts down the request.
    pub fn deny(self) -> upgrade::WriteOne<upgrade::Negotiated<TSubstream>> {
        let mut msg = CircuitRelay::new();
        msg.set_code(CircuitRelay_Status::STOP_RELAY_REFUSED);
        let msg_bytes = msg
            .write_to_bytes()
            .expect("all the mandatory fields are always filled; QED");
        upgrade::write_one(self.stream, msg_bytes)
    }
}

/// Future that accepts the request.
#[must_use = "futures do nothing unless polled"]
pub struct RelayDestinationAcceptFuture<TSubstream> {
    /// The inner stream.
    inner: Option<upgrade::Negotiated<TSubstream>>,
    /// The message to send to the remote.
    message: io::Cursor<Vec<u8>>,
}

impl<TSubstream> Future for RelayDestinationAcceptFuture<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type Item = upgrade::Negotiated<TSubstream>;
    type Error = Box<error::Error>; // TODO: change error

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        while self.message.remaining() != 0 {
            match self
                .inner
                .as_mut()
                .expect("Future is already finished")
                .write_buf(&mut self.message)?
            {
                Async::Ready(_) => (),
                Async::NotReady => return Ok(Async::NotReady),
            }
        }

        try_ready!(self
            .inner
            .as_mut()
            .expect("Future is already finished")
            .poll_flush());
        let stream = self.inner.take().expect("Future is already finished");
        Ok(Async::Ready(stream))
    }
}
