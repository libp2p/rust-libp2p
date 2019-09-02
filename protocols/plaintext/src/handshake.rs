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
use libp2p_core::PublicKey;
use log::{debug, trace};
use crate::structs_proto::Propose;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::length_delimited;
use tokio_io::codec::length_delimited::Framed;
use protobuf::Message;
use crate::error::PlainTextError;
use crate::PlainTextConfig;

struct HandShakeContext<T> {
    config: PlainTextConfig,
    state: T
}

// HandshakeContext<()> --with_local-> HandshakeContext<Local>
struct Local {
    // Our encoded local public key
    public_key_encoded: Vec<u8>,
    // Our local proposition's raw bytes:
    proposition_bytes: Vec<u8>,
}

// HandshakeContext<Local> --with_remote-> HandshakeContext<Remote>
struct Remote {
    local: Local,
    // The remote's proposition's raw bytes:
    proposition_bytes: BytesMut,
    // The remote's public key:
    public_key: PublicKey,
}

impl HandShakeContext<()> {
    fn new(config: PlainTextConfig) -> Self {
        Self {
            config,
            state: (),
        }
    }

    fn with_local(self) -> Result<HandShakeContext<Local>, PlainTextError> {
        let public_key_encoded = self.config.key.public().into_protobuf_encoding();
        let mut proposition = Propose::new();
        proposition.set_pubkey(public_key_encoded.clone());

        let proposition_bytes = proposition.write_to_bytes()?;

        Ok(HandShakeContext {
            config: self.config,
            state: Local {
                public_key_encoded,
                proposition_bytes,
            }
        })
    }
}

impl HandShakeContext<Local> {
    fn with_remote(self, proposition_bytes: BytesMut) -> Result<HandShakeContext<Remote>, PlainTextError> {
        let mut prop = match protobuf::parse_from_bytes::<Propose>(&proposition_bytes) {
            Ok(prop) => prop,
            Err(_) => {
                debug!("failed to parse remote's proposition protobuf message");
                return Err(PlainTextError::HandshakeParsingFailure);
            },
        };

        let public_key_encoded = prop.take_pubkey();
        let public_key = match PublicKey::from_protobuf_encoding(&public_key_encoded) {
            Ok(p) => p,
            Err(_) => {
                debug!("failed to parse remote's proposition's pubkey protobuf");
                return Err(PlainTextError::HandshakeParsingFailure);
            },
        };

        Ok(HandShakeContext {
            config: self.config,
            state: Remote {
                local: self.state,
                proposition_bytes,
                public_key,
            }
        })
    }
}

pub fn handshake<S>(socket: S, config: PlainTextConfig)
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
        .and_then(|context| {
            trace!("sending proposition to remote");
            socket.send(BytesMut::from(context.state.proposition_bytes.clone()))
                .from_err()
                .map(|s| (s, context))
        })
        .and_then(move |(socket, context)| {
            trace!("receiving the remote's proposition");
            socket.into_future()
                .map_err(|(e, _)| e.into())
                .and_then(move |(prop_raw, socket)| {
                    let context = match prop_raw {
                        Some(p) => context.with_remote(p)?,
                        None => {
                            debug!("unexpected eof while waiting for remote's proposition");
                            let err = IoError::new(IoErrorKind::BrokenPipe, "unexpected eof");
                            return Err(err.into());
                        }
                    };

                    trace!("received proposition from remote; pubkey = {:?}", context.state.public_key);
                    Ok((socket, context.state.public_key))
                })
        })
}
