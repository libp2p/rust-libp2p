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

use std::io::{Error as IoError, ErrorKind as IoErrorKind};

use asynchronous_codec::{Framed, FramedParts};
use bytes::Bytes;
use futures::prelude::*;
use libp2p_identity::{PeerId, PublicKey};

use crate::{
    error::{DecodeError, Error},
    proto::Exchange,
    Config,
};

pub(crate) async fn handshake<S>(socket: S, config: Config) -> Result<(S, PublicKey, Bytes), Error>
where
    S: AsyncRead + AsyncWrite + Send + Unpin,
{
    // The handshake messages all start with a variable-length integer indicating the size.
    let mut framed_socket = Framed::new(socket, quick_protobuf_codec::Codec::<Exchange>::new(100));

    tracing::trace!("sending exchange to remote");
    framed_socket
        .send(Exchange {
            id: Some(config.local_public_key.to_peer_id().to_bytes()),
            pubkey: Some(config.local_public_key.encode_protobuf()),
        })
        .await
        .map_err(DecodeError)?;

    tracing::trace!("receiving the remote's exchange");
    let public_key = match framed_socket
        .next()
        .await
        .transpose()
        .map_err(DecodeError)?
    {
        Some(remote) => {
            let public_key = PublicKey::try_decode_protobuf(&remote.pubkey.unwrap_or_default())?;
            let peer_id = PeerId::from_bytes(&remote.id.unwrap_or_default())?;

            if peer_id != public_key.to_peer_id() {
                return Err(Error::PeerIdMismatch);
            }

            public_key
        }
        None => {
            tracing::debug!("unexpected eof while waiting for remote's exchange");
            let err = IoError::new(IoErrorKind::BrokenPipe, "unexpected eof");
            return Err(err.into());
        }
    };

    tracing::trace!(?public_key, "received exchange from remote");

    let FramedParts {
        io,
        read_buffer,
        write_buffer,
        ..
    } = framed_socket.into_parts();
    assert!(write_buffer.is_empty());
    Ok((io, public_key, read_buffer.freeze()))
}
