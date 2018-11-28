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

use codec::Codec;
use libp2p_core::{Multiaddr, PeerId};
use futures::prelude::*;
use message::{CircuitRelay, CircuitRelay_Status};
use std::{error, io};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};
use utility::{Peer, status};      // TODO: move these here

/// Request from a remote for us to become a destination.
// TODO: debug
#[must_use = "A destination request should be either accepted or denied"]
pub struct RelayDestinationRequest<TStream> {
    /// The stream to the source.
    stream: Framed<TStream, Codec>,
    /// Source of the request.
    from: Peer,
}

impl<TStream> RelayDestinationRequest<TStream>
where TStream: AsyncRead + AsyncWrite
{
    /// Creates a `RelayDestinationRequest`.
    /// The `Framed` should be in the state right after pulling the remote's request message.
    #[inline]
    pub(crate) fn new(stream: Framed<TStream, Codec>, from: Peer) -> Self {
        RelayDestinationRequest {
            stream,
            from,
        }
    }

    /// Returns the peer id of the source that is being relayed.
    #[inline]
    pub fn source_id(&self) -> &PeerId {
        &self.from.id
    }

    /// Returns the addresses of the source that is being relayed.
    #[inline]
    pub fn source_addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.from.addrs.iter()
    }

    /// Accepts the request.
    ///
    /// The returned `Future` sends back a success message then returns the raw stream. This raw
    /// stream now points to the source (as retreived with `source_id()` and `source_addresses()`).
    pub fn accept(self) -> RelayDestinationAcceptFuture<TStream> {
        let message = status(CircuitRelay_Status::SUCCESS);
        RelayDestinationAcceptFuture {
            inner: Some(self.stream),
            message: Some(message),
        }
    }

    /// Refuses the request.
    ///
    /// The returned `Future` gracefully shuts down the request.
    pub fn deny(self) -> RelayDestinationDenyFuture<TStream> {
        // TODO: correct status
        let message = status(CircuitRelay_Status::STOP_RELAY_REFUSED);
        RelayDestinationDenyFuture {
            inner: self.stream,
            message: Some(message),
        }
    }
}

/// Future that accepts the request.
#[must_use = "futures do nothing unless polled"]
pub struct RelayDestinationAcceptFuture<TSubstream> {
    /// The inner stream.
    inner: Option<Framed<TSubstream, Codec>>,
    /// The message to send to the remote.
    message: Option<CircuitRelay>,
}

impl<TSubstream> Future for RelayDestinationAcceptFuture<TSubstream>
where TSubstream: AsyncRead + AsyncWrite,
{
    type Item = TSubstream;
    type Error = Box<error::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(message) = self.message.take() {
            match self.inner.as_mut().expect("Future is already finished").start_send(message)? {
                AsyncSink::Ready => (),
                AsyncSink::NotReady(message) => {
                    self.message = Some(message);
                    return Ok(Async::NotReady);
                },
            }
        }

        try_ready!(self.inner.as_mut().expect("Future is already finished").poll_complete());
        let stream = self.inner.take().expect("Future is already finished");
        Ok(Async::Ready(stream.into_inner()))       // TODO: may be wrong because of caching
    }
}

/// Future that refuses the request.
#[must_use = "futures do nothing unless polled"]
pub struct RelayDestinationDenyFuture<TSubstream> {
    /// The inner stream.
    inner: Framed<TSubstream, Codec>,
    /// The message to send to the remote.
    message: Option<CircuitRelay>,
}

impl<TSubstream> Future for RelayDestinationDenyFuture<TSubstream>
where TSubstream: AsyncRead + AsyncWrite,
{
    type Item = ();
    type Error = Box<error::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(message) = self.message.take() {
            match self.inner.start_send(message)? {
                AsyncSink::Ready => (),
                AsyncSink::NotReady(message) => {
                    self.message = Some(message);
                    return Ok(Async::NotReady);
                },
            }
        }

        try_ready!(self.inner.poll_complete());
        Ok(Async::Ready(()))
    }
}
