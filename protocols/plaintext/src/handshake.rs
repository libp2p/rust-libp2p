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
use crate::pb::keys::{PublicKey as PbPublicKey, KeyType};
use crate::pb::structs::Exchange;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::length_delimited;
use tokio_io::codec::length_delimited::Framed;
use protobuf::Message;
use crate::error::PlainTextError;
use crate::PlainText2Config;

struct HandShakeContext<T> {
    config: PlainText2Config,
    state: T
}

// HandshakeContext<()> --with_local-> HandshakeContext<Local>
struct Local {
    // Our local exchange's raw bytes:
    exchange_bytes: Vec<u8>,
}

// HandshakeContext<Local> --with_remote-> HandshakeContext<Remote>
struct Remote {
    // The remote's public key:
    peer_id: PeerId,
    // The remote's public key:
    public_key: PublicKey,
}

impl HandShakeContext<()> {
    fn new(config: PlainText2Config) -> Self {
        Self {
            config,
            state: (),
        }
    }

    fn with_local(self) -> Result<HandShakeContext<Local>, PlainTextError> {
        let mut exchange = Exchange::new();
        let mut pb_pubkey = PbPublicKey::new();
        pb_pubkey.set_Type(HandShakeContext::pubkey_to_keytype(&self.config.pubkey));
        pb_pubkey.set_Data(self.config.pubkey.clone().into_protobuf_encoding());
        exchange.set_pubkey(pb_pubkey);
        exchange.set_id(self.config.pubkey.clone().into_peer_id().into_bytes());

        let exchange_bytes = exchange.write_to_bytes()?;

        Ok(HandShakeContext {
            config: self.config,
            state: Local {
                exchange_bytes,
            }
        })
    }

    // See `libp2p_core::identity`
    #[cfg(not(any(target_os = "emscripten", target_os = "unknown")))]
    fn pubkey_to_keytype(pubkey: &PublicKey) -> KeyType {
        match pubkey {
            PublicKey::Ed25519(_) => KeyType::Ed25519,
            PublicKey::Rsa(_) => KeyType::RSA,
            PublicKey::Secp256k1(_) => KeyType::Secp256k1,
        }
    }

    // See `libp2p_core::identity`
    #[cfg(any(target_os = "emscripten", target_os = "unknown"))]
    fn pubkey_to_keytype(pubkey: &PublicKey) -> KeyType {
        match pubkey {
            PublicKey::Ed25519(_) => KeyType::Ed25519,
            PublicKey::Secp256k1(_) => KeyType::Secp256k1,
        }
    }
}

impl HandShakeContext<Local> {
    fn with_remote(self, exchange_bytes: BytesMut) -> Result<HandShakeContext<Remote>, PlainTextError> {
        let mut prop = match protobuf::parse_from_bytes::<Exchange>(&exchange_bytes) {
            Ok(prop) => prop,
            Err(_) => {
                debug!("failed to parse remote's exchange protobuf message");
                return Err(PlainTextError::HandshakeParsingFailure);
            },
        };

        let pb_pubkey = prop.take_pubkey();
        let public_key = match PublicKey::from_protobuf_encoding(pb_pubkey.get_Data()) {
            Ok(p) => p,
            Err(_) => {
                debug!("failed to parse remote's exchange's pubkey protobuf");
                return Err(PlainTextError::HandshakeParsingFailure);
            },
        };
        let peer_id = match PeerId::from_bytes(prop.take_id()) {
            Ok(p) => p,
            Err(_) => {
                debug!("failed to parse remote's exchange's id protobuf");
                return Err(PlainTextError::HandshakeParsingFailure);
            },
        };

        Ok(HandShakeContext {
            config: self.config,
            state: Remote {
                peer_id,
                public_key,
            }
        })
    }
}

pub fn handshake<S>(socket: S, config: PlainText2Config)
    -> impl Future<Item = (Framed<S, BytesMut>, PublicKey), Error = PlainTextError>
where
    S: AsyncRead + AsyncWrite + Send,
{
    let socket = length_delimited::Builder::new()
        .big_endian()
        .length_field_length(4)
        .new_framed(socket);

    future::ok::<_, PlainTextError>(HandShakeContext::new(config))
        .and_then(|context| {
            trace!("starting handshake");
            Ok(context.with_local()?)
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
                    Ok((socket, context))
                })
        })
        // Check the validity of the remote's `Exchange`.
        .and_then(|(socket, context)| {
            let is_valid = match context.state.peer_id.is_public_key(&context.state.public_key) {
                Some(b) => b,
                None => {
                    debug!("the remote's `PeerId`s hash algorithm is not unsupported");
                    return Err(PlainTextError::NoSupportIntersection)
                }
            };

            if !is_valid {
                debug!("The remote's `PeerId` of the exchange isn't consist with the remote public key");
                return Err(PlainTextError::PeerIdValidationFailed)
            }

            trace!("successfully validated the remote's peer ID");
            Ok((socket, context.state.public_key))
        })
}
