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

use crate::message_proto::circuit_relay;

use bytes::Bytes;
use futures::io::{AsyncRead, AsyncWrite};
use futures::channel::oneshot;
use libp2p_core::{multiaddr::Error as MultiaddrError, Multiaddr, PeerId};
use smallvec::SmallVec;
use std::io::{Error, IoSlice};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{convert::TryFrom, error, fmt};

/// Any message received on the wire whose length is superior to that will be refused and will
/// trigger an error.
const MAX_ACCEPTED_MESSAGE_LEN: usize = 1024;

const PROTOCOL_NAME: &[u8; 27] = b"/libp2p/relay/circuit/0.1.0";

// Source -> Relay
mod incoming_relay_request;
mod outgoing_relay_request;
pub use self::incoming_relay_request::{IncomingRelayRequest, IncomingRelayRequestError};
pub use self::outgoing_relay_request::{OutgoingRelayRequest, OutgoingRelayRequestError};

// Relay -> Destination
mod incoming_destination_request;
mod outgoing_destination_request;
pub use self::incoming_destination_request::{
    IncomingDestinationRequest, IncomingDestinationRequestError,
};
pub use self::outgoing_destination_request::{
    OutgoingDestinationRequest, OutgoingDestinationRequestError,
};

mod listen;
pub use self::listen::{RelayListen, RelayListenError, RelayRemoteRequest};

pub mod copy_future;

/// Strong typed version of a `CircuitRelay_Peer`.
///
/// Can be parsed from a `CircuitRelay_Peer` using the `TryFrom` trait.
//
// TODO: Is this used anywhere?
#[derive(Clone)]
pub(crate) struct Peer {
    pub(crate) peer_id: PeerId,
    pub(crate) addrs: SmallVec<[Multiaddr; 4]>,
}

impl TryFrom<circuit_relay::Peer> for Peer {
    type Error = PeerParseError;

    fn try_from(peer: circuit_relay::Peer) -> Result<Peer, Self::Error> {
        let circuit_relay::Peer { id, addrs } = peer;
        let peer_id = PeerId::from_bytes(&id).map_err(|_| PeerParseError::PeerIdParseError)?;
        let mut parsed_addrs = SmallVec::with_capacity(addrs.len());
        for addr in addrs.into_iter() {
            let addr = Multiaddr::try_from(addr).map_err(PeerParseError::MultiaddrParseError)?;
            parsed_addrs.push(addr);
        }
        Ok(Peer {
            peer_id,
            addrs: parsed_addrs,
        })
    }
}

/// Error while parsing information about a peer from a network message.
#[derive(Debug)]
pub enum PeerParseError {
    /// Failed to parse the identity of the peer.
    PeerIdParseError,
    /// Failed to parse one of the multiaddresses for the peer.
    MultiaddrParseError(MultiaddrError),
}

impl fmt::Display for PeerParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            PeerParseError::PeerIdParseError => write!(f, "Error while parsing the peer id"),
            PeerParseError::MultiaddrParseError(ref err) => {
                write!(f, "Error while parsing a multiaddress: {}", err)
            }
        }
    }
}

impl error::Error for PeerParseError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            PeerParseError::PeerIdParseError => None,
            PeerParseError::MultiaddrParseError(ref err) => Some(err),
        }
    }
}

/// A [`NegotiatedSubstream`] acting as a relayed [`Connection`].
//
// Might at first return data, that was already read during relay negotiation.
//
// TODO: Being generic over TSubstream needed here? Wouldn't NegotiatedSubstream do as well?
#[derive(Debug)]
pub struct Connection<TSubstream> {
    initial_data: Bytes,
    stream: TSubstream,

    /// Notifies the other side of the channel of this [`Connection`] being dropped.
    _notifier: oneshot::Sender<()>,
}

impl<TSubstream> Unpin for Connection<TSubstream> {}

impl<TSubstream> Connection<TSubstream> {
    fn new(initial_data: Bytes, stream: TSubstream, notifier: oneshot::Sender<()>) -> Self {
        Connection {
            initial_data,
            stream,

            _notifier: notifier,
        }
    }
}

impl<TSubstream: AsyncWrite + Unpin> AsyncWrite for Connection<TSubstream> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, Error>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Error>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Error>> {
        Pin::new(&mut self.stream).poll_close(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        bufs: &[IoSlice],
    ) -> Poll<Result<usize, Error>> {
        Pin::new(&mut self.stream).poll_write_vectored(cx, bufs)
    }
}

impl<TSubstream: AsyncRead + Unpin> AsyncRead for Connection<TSubstream> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, Error>> {
        if !self.initial_data.is_empty() {
            let n = std::cmp::min(self.initial_data.len(), buf.len());
            buf[0..n].copy_from_slice(&self.initial_data.as_ref()[..n]);
            let _ = self.initial_data.split_to(n);
            return Poll::Ready(Ok(n));
        }

        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}
