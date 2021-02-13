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
use crate::protocol::{Peer, MAX_ACCEPTED_MESSAGE_LEN};

use futures::{future::BoxFuture, prelude::*};
use futures_codec::Framed;
use libp2p_core::{Multiaddr, PeerId};
use prost::Message;
use std::io;
use std::io::Cursor;
use unsigned_varint::codec::UviBytes;

/// Request from a remote for us to become a destination.
///
/// If we take a situation where a *source* wants to talk to a *destination* through a *relay*, and
/// we are the *destination*, this struct is a message that the *relay* sent to us. The
/// parameters passed to `IncomingDestinationRequest::new()` are the information of the *source*.
///
/// If the upgrade succeeds, the substream is returned and we will receive data sent from the
/// source on it.
// TODO: debug
#[must_use = "A destination request should be either accepted or denied"]
pub struct IncomingDestinationRequest<TSubstream> {
    /// The stream to the source.
    stream: TSubstream,
    /// Source of the request.
    from: Peer,
}

impl<TSubstream> IncomingDestinationRequest<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    /// Creates a `IncomingDestinationRequest`.
    pub(crate) fn new(stream: TSubstream, from: Peer) -> Self {
        IncomingDestinationRequest {
            stream: stream,
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
    pub fn accept(
        self,
    ) -> BoxFuture<'static, Result<(PeerId, TSubstream), IncomingDestinationRequestError>> {
        let IncomingDestinationRequest { stream, from } = self;
        let msg = CircuitRelay {
            r#type: Some(circuit_relay::Type::Status.into()),
            src_peer: None,
            dst_peer: None,
            code: Some(circuit_relay::Status::Success.into()),
        };
        let mut msg_bytes = Vec::new();
        msg.encode(&mut msg_bytes)
            .expect("all the mandatory fields are always filled; QED");

        let mut codec = UviBytes::default();
        codec.set_max_len(MAX_ACCEPTED_MESSAGE_LEN);
        let mut substream = Framed::new(stream, codec);

        async move {
            substream.send(Cursor::new(msg_bytes)).await?;
            Ok((from.peer_id, substream.into_inner()))
        }
        .boxed()
    }

    /// Refuses the request.
    ///
    /// The returned `Future` gracefully shuts down the request.
    pub fn deny(self) -> BoxFuture<'static, Result<(), io::Error>> {
        let msg = CircuitRelay {
            r#type: Some(circuit_relay::Type::Status.into()),
            src_peer: None,
            dst_peer: None,
            code: Some(circuit_relay::Status::StopRelayRefused.into()),
        };
        let mut msg_bytes = Vec::new();
        msg.encode(&mut msg_bytes)
            .expect("all the mandatory fields are always filled; QED");

        let mut codec = UviBytes::default();
        codec.set_max_len(MAX_ACCEPTED_MESSAGE_LEN);
        let mut substream = Framed::new(self.stream, codec);

        async move {
            substream.send(Cursor::new(msg_bytes)).await?;
            Ok(())
        }
        .boxed()
    }
}

#[derive(Debug)]
pub enum IncomingDestinationRequestError {
    Io(std::io::Error),
}

impl From<std::io::Error> for IncomingDestinationRequestError {
    fn from(e: std::io::Error) -> Self {
        IncomingDestinationRequestError::Io(e)
    }
}
