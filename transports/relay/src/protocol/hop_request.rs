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
use std::error;
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};
use utility::{Peer, status};      // TODO: move these here

/// Request from a remote for us to relay communications to another node.
// TODO: debug
#[must_use = "A HOP request should be either accepted or denied"]
pub struct RelayHopRequest<TStream> {
    /// The stream to the source.
    stream: Framed<TStream, Codec>,
    /// Target of the request.
    dest: Peer,
}

impl<TStream> RelayHopRequest<TStream>
where TStream: AsyncRead + AsyncWrite
{
    /// Creates a `RelayHopRequest`.
    /// The `Framed` should be in the state right after pulling the remote's request message.
    #[inline]
    pub(crate) fn new(stream: Framed<TStream, Codec>, dest: Peer) -> Self {
        RelayHopRequest {
            stream,
            dest,
        }
    }

    /// Peer id of the node we should relay communications to.
    #[inline]
    pub fn target_id(&self) -> &PeerId {
        &self.dest.id
    }

    /// Returns the addresses of the target, as reported by the requester.
    #[inline]
    pub fn target_addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.dest.addrs.iter()
    }

    /// Accepts the request by providing a stream to the destination.
    ///
    /// The `dest_stream` should be a brand new dialing substream. This method will negotiate the
    /// `relay` protocol on it, send a relay message, and then relay to it the connection from the
    /// source.
    ///
    /// > **Note**: It is important that you process the `Future` that this method returns,
    /// >           otherwise the relaying will not work.
    pub fn fulfill<TDestStream>(self, dest_stream: TDestStream) -> RelayHopAcceptFuture<TDestStream>
    where TDestStream: AsyncRead + AsyncWrite
    {
        unimplemented!()        // TODO:
        /*RelayHopAcceptFuture {
            inner: Some(self.stream),
            message: Some(message),
        }

        let source_stream = self.stream;
        let stop = stop_message(&Peer::from_message(CircuitRelay_Peer::new()).unwrap(), &self.dest);
        upgrade::apply(dest_stream, TrivialUpgrade, Endpoint::Dialer)
            .and_then(|dest_stream| {
                // send STOP message to destination and expect back a SUCCESS message
                Io::new(dest_stream).send(stop)
                    .and_then(Io::recv)
                    .and_then(|(response, io)| {
                        let rsp = match response {
                            Some(m) => m,
                            None => return Err(io_err("no message from destination"))
                        };

                        if is_success(&rsp) {
                            Ok(io.into())
                        } else {
                            Err(io_err("no success response from relay"))
                        }
                    })
            })
            // signal success or failure to source
            .then(move |result| {
                match result {
                    Ok(c) => {
                        let msg = status(CircuitRelay_Status::SUCCESS);
                        A(source_stream.send(msg).map(|io| (io.into(), c)))
                    }
                    Err(e) => {
                        let msg = status(CircuitRelay_Status::HOP_CANT_DIAL_DST);
                        B(source_stream.send(msg).and_then(|_| Err(e)))
                    }
                }
            })
            // return future for bidirectional data transfer
            .and_then(move |(src, dst)| {
                let (src_r, src_w) = src.split();
                let (dst_r, dst_w) = dst.split();
                let a = copy::flushing_copy(src_r, dst_w).map(|_| ());
                let b = copy::flushing_copy(dst_r, src_w).map(|_| ());
                a.select(b).map(|_| ()).map_err(|(e, _)| e)
            })*/
    }

    /// Refuses the request.
    ///
    /// The returned `Future` gracefully shuts down the request.
    #[inline]
    pub fn deny(self) -> RelayHopDenyFuture<TStream> {
        // TODO: correct status
        let message = status(CircuitRelay_Status::HOP_CANT_RELAY_TO_SELF);
        RelayHopDenyFuture {
            inner: self.stream,
            message: Some(message),
        }
    }
}

/// Future that accepts the request.
#[must_use = "futures do nothing unless polled"]
pub struct RelayHopAcceptFuture<TSubstream> {
    /// The inner stream.
    inner: Option<Framed<TSubstream, Codec>>,
    /// The message to send to the remote.
    message: Option<CircuitRelay>,
}

impl<TSubstream> Future for RelayHopAcceptFuture<TSubstream>
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
pub struct RelayHopDenyFuture<TSubstream> {
    /// The inner stream.
    inner: Framed<TSubstream, Codec>,
    /// The message to send to the remote.
    message: Option<CircuitRelay>,
}

impl<TSubstream> Future for RelayHopDenyFuture<TSubstream>
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
