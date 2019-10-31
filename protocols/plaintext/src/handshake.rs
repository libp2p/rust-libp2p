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

use bytes::BytesMut;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use futures::Future;
use futures::future;
use futures::sink::Sink;
use futures::stream::Stream;
use libp2p_core::{PublicKey, PeerId};
use log::{debug, trace};
use crate::pb::structs::Exchange;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::length_delimited;
use tokio_io::codec::length_delimited::Framed;
use protobuf::Message;
use crate::error::PlainTextError;
use crate::PlainText2Config;

struct HandshakeContext<T> {
    config: PlainText2Config,
    state: T
}

// HandshakeContext<()> --with_local-> HandshakeContext<Local>
struct Local {
    // Our local exchange's raw bytes:
    exchange_bytes: Vec<u8>,
}

// HandshakeContext<Local> --with_remote-> HandshakeContext<Remote>
pub struct Remote {
    // The remote's peer ID:
    pub peer_id: PeerId,
    // The remote's public key:
    pub public_key: PublicKey,
}

impl HandshakeContext<Local> {
    fn new(config: PlainText2Config) -> Result<Self, PlainTextError> {
        let mut exchange = Exchange::new();
        exchange.set_id(config.local_public_key.clone().into_peer_id().into_bytes());
        exchange.set_pubkey(config.local_public_key.clone().into_protobuf_encoding());
        let exchange_bytes = exchange.write_to_bytes()?;

        Ok(Self {
            config,
            state: Local {
                exchange_bytes
            }
        })
    }

    fn with_remote(self, exchange_bytes: BytesMut) -> Result<HandshakeContext<Remote>, PlainTextError> {
        let mut prop = match protobuf::parse_from_bytes::<Exchange>(&exchange_bytes) {
            Ok(prop) => prop,
            Err(e) => {
                debug!("failed to parse remote's exchange protobuf message");
                return Err(PlainTextError::InvalidPayload(Some(e)));
            },
        };

        let pb_pubkey = prop.take_pubkey();
        let public_key = match PublicKey::from_protobuf_encoding(pb_pubkey.as_slice()) {
            Ok(p) => p,
            Err(_) => {
                debug!("failed to parse remote's exchange's pubkey protobuf");
                return Err(PlainTextError::InvalidPayload(None));
            },
        };
        let peer_id = match PeerId::from_bytes(prop.take_id()) {
            Ok(p) => p,
            Err(_) => {
                debug!("failed to parse remote's exchange's id protobuf");
                return Err(PlainTextError::InvalidPayload(None));
            },
        };

        // Check the validity of the remote's `Exchange`.
        if peer_id != public_key.clone().into_peer_id() {
            debug!("The remote's `PeerId` of the exchange isn't consist with the remote public key");
            return Err(PlainTextError::InvalidPeerId)
        }

        Ok(HandshakeContext {
            config: self.config,
            state: Remote {
                peer_id,
                public_key,
            }
        })
    }
}

pub fn handshake<S>(socket: S, config: PlainText2Config)
    -> impl Future<Item = (Framed<S, BytesMut>, Remote), Error = PlainTextError>
where
    S: AsyncRead + AsyncWrite + Send,
{
    let socket = length_delimited::Builder::new()
        .big_endian()
        .length_field_length(4)
        .new_framed(socket);

    future::ok::<_, PlainTextError>(())
        .and_then(|_| {
            trace!("starting handshake");
            Ok(HandshakeContext::new(config)?)
        })
        // Send our local `Exchange`.
        .and_then(|context| {
            trace!("sending exchange to remote");
            socket.send(BytesMut::from(context.state.exchange_bytes.clone()))
                .from_err()
                .map(|s| (s, context))
        })
        // Receive the remote's `Exchange`.
        .and_then(move |(socket, context)| {
            trace!("receiving the remote's exchange");
            socket.into_future()
                .map_err(|(e, _)| e.into())
                .and_then(move |(prop_raw, socket)| {
                    let context = match prop_raw {
                        Some(p) => context.with_remote(p)?,
                        None => {
                            debug!("unexpected eof while waiting for remote's exchange");
                            let err = IoError::new(IoErrorKind::BrokenPipe, "unexpected eof");
                            return Err(err.into());
                        }
                    };

                    trace!("received exchange from remote; pubkey = {:?}", context.state.public_key);
                    Ok((socket, context.state))
                })
        })
}
