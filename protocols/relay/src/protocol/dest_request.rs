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

use crate::message_proto::{circuit_relay, CircuitRelay};
use crate::protocol::Peer;
use bytes::Buf as _;
use futures::{future::BoxFuture, prelude::*, ready};
use futures_codec::Framed;
use libp2p_core::{upgrade, Multiaddr, PeerId};
use libp2p_swarm::NegotiatedSubstream;
use prost::Message;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::{error, io};
use unsigned_varint::codec::UviBytes;

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
    // TODO: Cleanup arc mutex.
    stream: Arc<Mutex<Option<TSubstream>>>,
    /// Source of the request.
    from: Peer,
}

impl<TSubstream> Clone for RelayDestinationRequest<TSubstream> {
    fn clone(&self) -> Self {
        RelayDestinationRequest {
            stream: self.stream.clone(),
            from: self.from.clone(),
        }
    }
}

impl<TSubstream> RelayDestinationRequest<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    /// Creates a `RelayDestinationRequest`.
    pub(crate) fn new(stream: TSubstream, from: Peer) -> Self {
        RelayDestinationRequest {
            stream: Arc::new(Mutex::new(Some(stream))),
            from,
        }
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
    pub fn accept(self) -> impl Future<Output = Result<TSubstream, Box<dyn error::Error + 'static>>> {
        let msg = CircuitRelay {
            r#type: None,
            src_peer: None,
            dst_peer: None,
            code: Some(circuit_relay::Status::Success.into()),
        };
        let mut msg_bytes = Vec::new();
        // TODO: Handl2
        msg.encode(&mut msg_bytes)
            .expect("all the mandatory fields are always filled; QED");

        let codec = UviBytes::default();
        // TODO: Do we need this?
        // codec.set_max_len(self.max_packet_size);
        let mut substream = Framed::new(self.stream.lock().unwrap().take().unwrap(), codec);

        async move {
            substream.send(std::io::Cursor::new(msg_bytes)).await.unwrap();
            Ok(substream.into_inner())
        }.boxed()
    }

    /// Refuses the request.
    ///
    /// The returned `Future` gracefully shuts down the request.
    pub fn deny(self) -> BoxFuture<'static, Result<(), io::Error>> {
        let msg = CircuitRelay {
            r#type: None,
            src_peer: None,
            dst_peer: None,
            code: Some(circuit_relay::Status::StopRelayRefused.into()),
        };
        let mut msg_bytes = Vec::new();
        // TODO: Handl2
        msg.encode(&mut msg_bytes)
            .expect("all the mandatory fields are always filled; QED");

        // async {
        //     upgrade::write_one(&mut self.stream, msg_bytes).await?;
        // }.boxed()

        unimplemented!();
    }
}

/// Future that accepts the request.
#[must_use = "futures do nothing unless polled"]
pub struct RelayDestinationAcceptFuture<TSubstream> {
    /// The inner stream.
    // TODO: Cleanup arc mutex
    inner: Option<Arc<Mutex<TSubstream>>>,
    /// The message to send to the remote.
    message: io::Cursor<Vec<u8>>,
}

impl<TSubstream> Unpin for RelayDestinationAcceptFuture<TSubstream> {}

impl<TSubstream> Future for RelayDestinationAcceptFuture<TSubstream> {
    type Output = Result<NegotiatedSubstream, Box<dyn error::Error>>; // TODO: change error

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unimplemented!();
        // while self.message.remaining() != 0 {
        //     match Pin::new(self
        //         .inner
        //         .as_mut()
        //         .expect("Future is already finished"))
        //         .poll_write(cx, &self.message)?
        //     {
        //         // TODO: Handle error and partial send.
        //         Poll::Ready(_) => (),
        //         Poll::Pending => return Ok(Poll::Pending),
        //     }
        // }

        // ready!(self
        //     .inner
        //     .as_mut()
        //     .expect("Future is already finished")
        //     .poll_flush());
        // let stream = self.inner.take().expect("Future is already finished");
        // Poll::Ready(Ok(stream))
    }
}
