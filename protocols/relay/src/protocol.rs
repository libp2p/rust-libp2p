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

use crate::message::CircuitRelay_Peer;
use bytes::Buf as _;
use futures::prelude::*;
use libp2p_core::{multiaddr::Error as MultiaddrError, upgrade, Multiaddr, PeerId};
use smallvec::SmallVec;
use std::{convert::TryFrom, error, fmt, io, mem};
use tokio_io::{AsyncRead, AsyncWrite};

/// Any message received on the wire whose length is superior to that will be refused and will
/// trigger an error.
const MAX_ACCEPTED_MESSAGE_LEN: usize = 1024;

mod dest_request;
mod hop_request;
mod listen;
mod relay_request;
mod target_open;

pub use self::dest_request::{RelayDestinationAcceptFuture, RelayDestinationRequest};
pub use self::hop_request::{RelayHopAcceptFuture, RelayHopRequest}; // TODO: one missing
pub use self::listen::{RelayListen, RelayListenError, RelayListenFuture, RelayRemoteRequest};
pub use self::relay_request::RelayProxyRequest; // TODO:
pub use self::target_open::RelayTargetOpen;

/// Strong typed version of a `CircuitRelay_Peer`.
///
/// Can be parsed from a `CircuitRelay_Peer` using the `TryFrom` trait.
pub(crate) struct Peer {
    pub(crate) peer_id: PeerId,
    pub(crate) addrs: SmallVec<[Multiaddr; 4]>,
}

impl TryFrom<CircuitRelay_Peer> for Peer {
    type Error = PeerParseError;

    fn try_from(mut peer: CircuitRelay_Peer) -> Result<Peer, Self::Error> {
        let peer_id =
            PeerId::from_bytes(peer.take_id()).map_err(|_| PeerParseError::PeerIdParseError)?;
        let mut addrs = SmallVec::with_capacity(peer.get_addrs().len());
        for addr in peer.take_addrs().into_iter() {
            let addr = Multiaddr::try_from(addr).map_err(PeerParseError::MultiaddrParseError)?;
            addrs.push(addr);
        }
        Ok(Peer { peer_id, addrs })
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

pub enum SendReadError {}

impl From<io::Error> for SendReadError {
    fn from(err: io::Error) -> SendReadError {
        unimplemented!()
    }
}

impl From<upgrade::ReadOneError> for SendReadError {
    fn from(err: upgrade::ReadOneError) -> SendReadError {
        unimplemented!()
    }
}

/// Builds a future that writes the given bytes to the substream, then reads a relay response,
/// validates it, then returns the substream.
pub fn send_read<TSubstream, TUserData>(
    substream: upgrade::Negotiated<TSubstream>,
    message: Vec<u8>,
    user_data: TUserData,
) -> SendReadFuture<TSubstream, TUserData> {
    SendReadFuture {
        user_data: Some(user_data),
        inner: FutureInner::Sending {
            substream,
            message: io::Cursor::new(message),
        },
    }
}

/// Future that writes the given bytes to the substream, then reads a relay response, validates it,
/// then returns the substream.
#[must_use]
pub struct SendReadFuture<TSubstream, TUserData> {
    inner: FutureInner<TSubstream>,
    user_data: Option<TUserData>,
}

enum FutureInner<TSubstream> {
    /// We are trying to push the data to the substream.
    Sending {
        substream: upgrade::Negotiated<TSubstream>,
        message: io::Cursor<Vec<u8>>,
    },

    /// We are trying to flush the substream.
    Flushing(upgrade::Negotiated<TSubstream>),

    /// We are trying to read the response from the remote.
    Reading(
        upgrade::ReadRespond<
            upgrade::Negotiated<TSubstream>,
            (),
            fn(
                upgrade::Negotiated<TSubstream>,
                Vec<u8>,
                (),
            ) -> Result<upgrade::Negotiated<TSubstream>, SendReadError>,
        >,
    ),

    /// Something bad happened during the last cycle and we cannot continue.
    Poisoned,
}

impl<TSubstream, TUserData> Future for SendReadFuture<TSubstream, TUserData>
where
    TSubstream: AsyncRead + AsyncWrite,
{
    type Item = (upgrade::Negotiated<TSubstream>, TUserData);
    type Error = SendReadError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, FutureInner::Poisoned) {
                FutureInner::Sending {
                    mut substream,
                    mut message,
                } => {
                    match substream.write_buf(&mut message)? {
                        Async::Ready(_) => {}
                        Async::NotReady => {
                            self.inner = FutureInner::Sending { substream, message };
                            return Ok(Async::NotReady);
                        }
                    }

                    if message.remaining() == 0 {
                        self.inner = FutureInner::Flushing(substream);
                    } else {
                        self.inner = FutureInner::Sending { substream, message };
                    }
                }

                FutureInner::Flushing(mut substream) => match substream.poll_flush()? {
                    Async::Ready(()) => {
                        panic!() //let fut = upgrade::read_respond(substream, MAX_ACCEPTED_MESSAGE_LEN, (), |_, _, _| panic!());
                                 //self.inner = FutureInner::Reading(fut);
                    }
                    Async::NotReady => {
                        self.inner = FutureInner::Flushing(substream);
                        return Ok(Async::NotReady);
                    }
                },

                FutureInner::Reading(mut fut) => match fut.poll()? {
                    Async::Ready(out) => {
                        return Ok(Async::Ready((out, self.user_data.take().unwrap())))
                    }
                    Async::NotReady => {
                        self.inner = FutureInner::Reading(fut);
                        return Ok(Async::NotReady);
                    }
                },

                FutureInner::Poisoned => panic!("Poisoned state"),
            };
        }
    }
}
