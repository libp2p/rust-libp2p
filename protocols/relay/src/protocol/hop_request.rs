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

use crate::copy::Copy;
use crate::message_proto::{CircuitRelay, circuit_relay::Status};
use crate::protocol::Peer;
use bytes::Buf as _;
use futures::{prelude::*, future::BoxFuture, ready};
use libp2p_core::{upgrade, Multiaddr, PeerId};
use libp2p_swarm::NegotiatedSubstream;
use std::{error, io};
use std::task::{Context, Poll};
use std::pin::Pin;
use prost::Message;

/// Request from a remote for us to relay communications to another node.
///
/// If we take a situation where a *source* wants to talk to a *destination* through a *relay*, and
/// we are the *relay*, this struct is a message that the *source* sent to us. The parameters
/// passed to `RelayHopRequest::new()` are the information of the *destination*.
///
/// If the upgrade succeeds, the substream is returned and we will receive data sent from the
/// source on it. This data must be transmitted to the destination.
// TODO: debug
#[must_use = "A HOP request should be either accepted or denied"]
pub struct RelayHopRequest<TSubstream> {
    /// The stream to the source.
    stream: TSubstream,
    /// Target of the request.
    dest: Peer,
}

impl<TSubstream> RelayHopRequest<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    /// Creates a `RelayHopRequest`.
    pub(crate) fn new(stream: TSubstream, dest: Peer) -> Self {
        RelayHopRequest { stream, dest }
    }

    /// Peer id of the node we should relay communications to.
    pub fn destination_id(&self) -> &PeerId {
        &self.dest.peer_id
    }

    /// Returns the addresses of the target, as reported by the requester.
    pub fn destination_addresses(&self) -> impl Iterator<Item = &Multiaddr> {
        self.dest.addrs.iter()
    }

    /// Accepts the request by providing a stream to the destination.
    ///
    /// The `dest_stream` should be a brand new dialing substream. This method will negotiate the
    /// `relay` protocol on it, send a relay message, and then relay to it the connection from the
    /// source.
    ///
    /// The future that this method returns succeeds after the negotiation has succeeded. It
    /// returns another future that will copy the data.
    pub fn fulfill<TDestSubstream>(
        self,
        dest_stream: TDestSubstream,
    ) -> RelayHopAcceptFuture<NegotiatedSubstream, NegotiatedSubstream>
    where
        TDestSubstream: AsyncRead + AsyncWrite + Send + Unpin,
    {
        unimplemented!() // TODO:
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
    pub fn deny(mut self) -> BoxFuture<'static, Result<(), io::Error>> {
        let msg = CircuitRelay {
            r#type: None,
            code: Some(Status::StopRelayRefused.into()),
            src_peer: None,
            dst_peer: None,
        };
        let mut encoded_msg = Vec::new();
        // TODO: Rework.
        msg.encode(&mut encoded_msg).expect("all the mandatory fields are always filled; QED");
        unimplemented!();
        // Box::pin(async {
        //     upgrade::write_one(&mut self.stream, encoded_msg).await
        // })
    }
}

/// Future that accepts the request.
#[must_use = "futures do nothing unless polled"]
pub struct RelayHopAcceptFuture<TSrcSubstream, TDestSubstream> {
    /// The inner stream.
    inner: Option<TSrcSubstream>,
    /// The message to send to the remote.
    message: io::Cursor<Vec<u8>>,
    dest_substream: TDestSubstream,
}

impl<TSrcSubstream, TDestSubstream> Unpin for RelayHopAcceptFuture<TSrcSubstream, TDestSubstream> {}

impl<TSrcSubstream, TDestSubstream> Future for RelayHopAcceptFuture<TSrcSubstream, TDestSubstream>
where
    TSrcSubstream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    TDestSubstream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = Result<Copy<TSrcSubstream, TDestSubstream>, Box<dyn error::Error>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        // TODO: Fix.
        Poll::Pending
        // while self.message.remaining() != 0 {
        //     match self
        //         .inner
        //         .as_mut()
        //         .expect("Future is already finished")
        //         .poll_write(&mut self.message, cx)
        //     {
        //         // TODO: Handle error and partial send.
        //         Poll::Ready(_) => (),
        //         Poll::Pending => return Poll::Pending,
        //     }
        // }

        // ready!(self
        //     .inner
        //     .as_mut()
        //     .expect("Future is already finished")
        //     .poll_flush(cx));
        // let stream = self.inner.take().expect("Future is already finished");
        // panic!()
    }
}
