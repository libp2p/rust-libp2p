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

use futures::{future::{self, Either}, prelude::*};
use message::{CircuitRelay, CircuitRelay_Peer, CircuitRelay_Status, CircuitRelay_Type};
use multiaddr::{Protocol, Multiaddr};
use peerstore::PeerId;
use protobuf::{self, Message};
use std::{io, error::Error, iter::FromIterator};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec;

pub(crate) fn is_success(msg: &CircuitRelay) -> bool {
    msg.get_field_type() == CircuitRelay_Type::STATUS
        && msg.get_code() == CircuitRelay_Status::SUCCESS
}

pub(crate) fn status(s: CircuitRelay_Status) -> CircuitRelay {
    let mut msg = CircuitRelay::new();
    msg.set_field_type(CircuitRelay_Type::STATUS);
    msg.set_code(s);
    msg
}

pub(crate) struct Io<T> {
    codec: Framed<T, codec::UviBytes<Vec<u8>>>,
}

impl<T: AsyncRead + AsyncWrite> Io<T> {
    pub(crate) fn new(c: T) -> Io<T> {
        Io {
            codec: Framed::new(c, codec::UviBytes::default()),
        }
    }

    pub(crate) fn into(self) -> T {
        self.codec.into_inner()
    }
}

impl<T> Io<T>
where
    T: AsyncRead + AsyncWrite + 'static,
{
    pub(crate) fn send(self, msg: CircuitRelay) -> impl Future<Item=Self, Error=io::Error> {
        trace!("sending protocol message: type={:?}, code={:?}",
               msg.get_field_type(),
               msg.get_code());
        let pkg = match msg.write_to_bytes() {
            Ok(p) => p,
            Err(e) => return Either::A(future::err(io_err(e)))
        };
        Either::B(self.codec.send(pkg).map(|codec| Io { codec }))
    }

    pub(crate) fn recv(self) -> impl Future<Item=(Option<CircuitRelay>, Self), Error=io::Error> {
        self.codec
            .into_future()
            .map_err(|(e, _)| io_err(e))
            .and_then(|(pkg, codec)| {
                if let Some(ref p) = pkg {
                    protobuf::parse_from_bytes(p)
                        .map(|msg: CircuitRelay| {
                            trace!("received protocol message: type={:?}, code={:?}",
                                   msg.get_field_type(),
                                   msg.get_code());
                            (Some(msg), Io { codec })
                        })
                        .map_err(io_err)
                } else {
                    Ok((None, Io { codec }))
                }
            })
    }
}

pub(crate) enum RelayAddr {
    Address { relay: Option<Peer>, dest: Peer },
    Malformed,
    Multihop, // Unsupported
}

impl RelayAddr {
    // Address format: [<relay>]/p2p-circuit/<destination>
    pub(crate) fn parse(addr: &Multiaddr) -> RelayAddr {
        let mut iter = addr.iter().peekable();

        let relay = if let Some(&Protocol::P2pCircuit) = iter.peek() {
            None // Address begins with "p2p-circuit", i.e. no relay is specified.
        } else {
            let prefix = iter.by_ref().take_while(|p| *p != Protocol::P2pCircuit);
            match Peer::from(Multiaddr::from_iter(prefix)) {
                None => return RelayAddr::Malformed,
                peer => peer,
            }
        };

        // After the (optional) relay, "p2p-circuit" is expected.
        if Some(Protocol::P2pCircuit) != iter.next() {
            return RelayAddr::Malformed;
        }

        let dest = {
            let suffix = iter.by_ref().take_while(|p| *p != Protocol::P2pCircuit);
            match Peer::from(Multiaddr::from_iter(suffix)) {
                None => return RelayAddr::Malformed,
                Some(p) => p,
            }
        };

        if iter.next().is_some() {
            return RelayAddr::Multihop;
        }

        RelayAddr::Address { relay, dest }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Peer {
    pub(crate) id: PeerId,
    pub(crate) addrs: Vec<Multiaddr>,
}

impl Peer {
    pub(crate) fn from(mut addr: Multiaddr) -> Option<Peer> {
        match addr.pop() {
            Some(Protocol::P2p(id)) => {
                PeerId::from_multihash(id).ok().map(|pid| {
                    if addr.iter().count() == 0 {
                        Peer {
                            id: pid,
                            addrs: Vec::new(),
                        }
                    } else {
                        Peer {
                            id: pid,
                            addrs: vec![addr],
                        }
                    }
                })
            }
            _ => None,
        }
    }

    pub(crate) fn from_message(mut m: CircuitRelay_Peer) -> Option<Peer> {
        let pid = PeerId::from_bytes(m.take_id()).ok()?;
        let mut addrs = Vec::new();
        for a in m.take_addrs().into_iter() {
            if let Ok(ma) = Multiaddr::from_bytes(a) {
                addrs.push(ma)
            }
        }
        Some(Peer { id: pid, addrs })
    }
}

pub(crate) fn io_err<E>(e: E) -> io::Error
where
    E: Into<Box<Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::Other, e)
}
