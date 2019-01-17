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
use libp2p_core::upgrade;
use futures::prelude::*;
use message::CircuitRelay_Type;
use protocol::{dest_request::RelayDestinationRequest, hop_request::RelayHopRequest};
use std::{error, fmt, io, iter};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};
use utility::Peer;      // TODO: move

/// Configuration for an inbound upgrade that handles requests from the remote for the relay
/// protocol.
#[derive(Debug, Clone)]
pub struct RelayListen {
}

pub enum RelayRemoteRequest<TSubstream> {
    /// We have been asked to become a destination.
    DestinationRequest(RelayDestinationRequest<TSubstream>),
    /// We have been asked to relay communications to another node.
    HopRequest(RelayHopRequest<TSubstream>),
}

impl RelayListen {
    /// Builds a new `RelayListen` with default options.
    pub fn new() -> RelayListen {
        RelayListen {}
    }
}

impl upgrade::UpgradeInfo for RelayListen {
    type UpgradeId = ();
    type NamesIter = iter::Once<(Bytes, Self::UpgradeId)>;

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/libp2p/relay/circuit/0.1.0"), ()))
    }
}

impl<TSubstream> upgrade::InboundUpgrade<TSubstream> for RelayListen
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type Output = RelayRemoteRequest<TSubstream>;
    type Error = RelayListenError;
    type Future = RelayListenFuture<TSubstream>;

    fn upgrade_inbound(self, conn: TSubstream, _: ()) -> Self::Future {
        let inner = Framed::new(conn, Codec::new());
        RelayListenFuture {
            inner: Some(inner)
        }
    }
}

/// Future that negotiates an inbound substream for the relay protocol.
#[must_use = "futures do nothing unless polled"]
pub struct RelayListenFuture<TSubstream> {
    /// Inner stream.
    inner: Option<Framed<TSubstream, Codec>>,
}

impl<TSubstream> Future for RelayListenFuture<TSubstream>
where TSubstream: AsyncRead + AsyncWrite,
{
    type Item = RelayRemoteRequest<TSubstream>;
    type Error = RelayListenError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let mut msg = match try_ready!(self.inner.as_mut().expect("Future already finished").poll()) {
            Some(msg) => msg,
            None => return Err(RelayListenError::UnexpectedEof),
        };

        match msg.get_field_type() {
            CircuitRelay_Type::HOP => {
                let peer = Peer::from_message(msg.take_dstPeer());
                let rq = RelayHopRequest::new(self.inner.take().unwrap(), peer.unwrap());       // TODO:
                Ok(Async::Ready(RelayRemoteRequest::HopRequest(rq)))
            },
            CircuitRelay_Type::STOP => {
                let peer = Peer::from_message(msg.take_srcPeer());
                let rq = RelayDestinationRequest::new(self.inner.take().unwrap(), peer.unwrap());       // TODO:
                Ok(Async::Ready(RelayRemoteRequest::DestinationRequest(rq)))
            },
            _ => Err(RelayListenError::InvalidMessageTy)
        }
    }
}

/// Error while upgrading with a `RelayListen`.
#[derive(Debug)]
pub enum RelayListenError {
    /// Raw error on the substream.
    Io(Box<error::Error>),
    /// The remote closed the connection before we could get a message.
    UnexpectedEof,
    /// Received a message invalid in this context.
    InvalidMessageTy,
}

impl fmt::Display for RelayListenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RelayListenError::Io(ref err) => {
                write!(f, "Raw error on the substream: {:?}", err)
            },
            RelayListenError::UnexpectedEof => {
                write!(f, "The remote closed the connection before we could get a message")
            },
            RelayListenError::InvalidMessageTy => {
                write!(f, "Received a message invalid in this context.")
            },
        }
    }
}

impl error::Error for RelayListenError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            RelayListenError::Io(ref err) => Some(&**err),
            RelayListenError::UnexpectedEof => None,
            RelayListenError::InvalidMessageTy => None,
        }
    }
}

impl From<Box<error::Error>> for RelayListenError {
    #[inline]
    fn from(err: Box<error::Error>) -> Self {
        RelayListenError::Io(err)
    }
}
