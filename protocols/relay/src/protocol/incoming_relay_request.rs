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

use crate::message_proto::{circuit_relay, circuit_relay::Status, CircuitRelay};
use crate::protocol::{MAX_ACCEPTED_MESSAGE_LEN, Peer};

use futures::future::{BoxFuture, Either, select};
use futures::io::{Error, ErrorKind};
use futures::prelude::*;
use futures_codec::Framed;
use libp2p_core::{Multiaddr, PeerId};

use prost::Message;

use std::sync::{Arc, Mutex};

use std::{io};
use unsigned_varint::codec::UviBytes;

/// Request from a remote for us to relay communications to another node.
///
/// If we take a situation where a *source* wants to talk to a *destination* through a *relay*, and
/// we are the *relay*, this struct is a message that the *source* sent to us. The parameters
/// passed to `IncomingRelayRequest::new()` are the information of the *destination*.
///
/// If the upgrade succeeds, the substream is returned and we will receive data sent from the
/// source on it. This data must be transmitted to the destination.
// TODO: debug
#[must_use = "A HOP request should be either accepted or denied"]
pub struct IncomingRelayRequest<TSubstream> {
    /// The stream to the source.
    // TODO: Clean up mutex arc
    stream: Arc<Mutex<Option<TSubstream>>>,
    /// Target of the request.
    dest: Peer,
}

impl<TSubstream> Clone for IncomingRelayRequest<TSubstream> {
    fn clone(&self) -> Self {
        IncomingRelayRequest {
            stream: self.stream.clone(),
            dest: self.dest.clone(),
        }
    }
}

impl<TSubstream> IncomingRelayRequest<TSubstream>
where
    TSubstream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    /// Creates a `IncomingRelayRequest`.
    pub(crate) fn new(stream: TSubstream, dest: Peer) -> Self {
        IncomingRelayRequest {
            stream: Arc::new(Mutex::new(Some(stream))),
            dest,
        }
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
    ) -> BoxFuture<'static, ()>
    where
        TDestSubstream: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
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
        let mut substream = Framed::new(
            self.stream.lock().unwrap().take().unwrap(),
            codec,
        );

        async move {
            substream.send(std::io::Cursor::new(msg_bytes)).await.unwrap();

            let (from_source, mut to_source) = substream.into_inner().split();
            let (from_destination, mut to_destination) = dest_stream.split();

            let source_to_destination = futures::io::copy(from_source, &mut to_destination);
            let destination_to_source = futures::io::copy(from_destination, &mut to_source);

            match select(source_to_destination, destination_to_source).await {
                // Destination substream closed.
                Either::Left((Err(e), _)) if e.kind() != ErrorKind::UnexpectedEof => panic!(e),
                // Source substream closed.
                Either::Right((Err(e), _)) if e.kind() != ErrorKind::UnexpectedEof => panic!(e),
                _ => {},
            }
        }.boxed()
    }

    /// Refuses the request.
    ///
    /// The returned `Future` gracefully shuts down the request.
    pub fn deny(self) -> BoxFuture<'static, Result<(), io::Error>> {
        let msg = CircuitRelay {
            r#type: None,
            code: Some(Status::StopRelayRefused.into()),
            src_peer: None,
            dst_peer: None,
        };
        let mut encoded_msg = Vec::new();
        msg.encode(&mut encoded_msg)
            .expect("all the mandatory fields are always filled; QED");
        unimplemented!();
    }
}
