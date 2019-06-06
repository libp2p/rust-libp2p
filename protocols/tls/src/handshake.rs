use futures::*;
use tokio_io::codec::length_delimited;
use crate::codec::{FullCodec, full_codec};
use tokio_io::{AsyncRead, AsyncWrite};
use crate::error::TlsError;
use crate::{TlsConfig};
use libp2p_core::PublicKey;
use bytes::BytesMut;

pub fn handshake<S>(socket: S, config: TlsConfig) -> impl Future<Item = (FullCodec<S>, PublicKey), Error = TlsError>
    where
        S: AsyncRead + AsyncWrite + Send,
{
    let socket = length_delimited::Builder::new()
        .big_endian()
        .length_field_length(4)
        .new_framed(socket);

    //future::ok((full_codec(socket), config.key.public()))

    socket.send(BytesMut::from(config.key.public().into_protobuf_encoding()))
        .from_err()
        .and_then(|s| {
            s.into_future()
                .map_err(|(e, _)| e.into())
                .and_then(move |(bytes, s)| {
                    let pubkey = PublicKey::from_protobuf_encoding(&bytes.unwrap());
                    Ok((full_codec(s), pubkey.unwrap()))
                })
        })
}