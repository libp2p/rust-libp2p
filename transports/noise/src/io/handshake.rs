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

//! Noise protocol handshake I/O.

mod proto {
    #![allow(unreachable_pub)]
    include!("../generated/mod.rs");
    pub use self::payload::proto::NoiseHandshakePayload;
}

use crate::io::{framed::NoiseFramed, Output};
use crate::protocol::{KeypairIdentity, STATIC_KEY_DOMAIN};
use crate::{DecodeError, Error};
use bytes::Bytes;
use futures::prelude::*;
use libp2p_identity as identity;
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use std::io;

//////////////////////////////////////////////////////////////////////////////
// Internal

/// Handshake state.
pub(crate) struct State<T> {
    /// The underlying I/O resource.
    io: NoiseFramed<T, snow::HandshakeState>,
    /// The associated public identity of the local node's static DH keypair,
    /// which can be sent to the remote as part of an authenticated handshake.
    identity: KeypairIdentity,
    /// The received signature over the remote's static DH public key, if any.
    dh_remote_pubkey_sig: Option<Vec<u8>>,
    /// The known or received public identity key of the remote, if any.
    id_remote_pubkey: Option<identity::PublicKey>,
}

impl<T> State<T> {
    /// Initializes the state for a new Noise handshake, using the given local
    /// identity keypair and local DH static public key. The handshake messages
    /// will be sent and received on the given I/O resource and using the
    /// provided session for cryptographic operations according to the chosen
    /// Noise handshake pattern.

    pub(crate) fn new(
        io: T,
        session: snow::HandshakeState,
        identity: KeypairIdentity,
        expected_remote_key: Option<identity::PublicKey>,
    ) -> Self {
        Self {
            identity,
            io: NoiseFramed::new(io, session),
            dh_remote_pubkey_sig: None,
            id_remote_pubkey: expected_remote_key,
        }
    }
}

impl<T> State<T> {
    /// Finish a handshake, yielding the established remote identity and the
    /// [`Output`] for communicating on the encrypted channel.
    pub(crate) fn finish(self) -> Result<(identity::PublicKey, Output<T>), Error> {
        let (pubkey, io) = self.io.into_transport()?;

        let id_pk = self
            .id_remote_pubkey
            .ok_or_else(|| Error::AuthenticationFailed)?;

        let is_valid_signature = self.dh_remote_pubkey_sig.as_ref().map_or(false, |s| {
            id_pk.verify(&[STATIC_KEY_DOMAIN.as_bytes(), pubkey.as_ref()].concat(), s)
        });

        if !is_valid_signature {
            return Err(Error::BadSignature);
        }

        Ok((id_pk, io))
    }
}

//////////////////////////////////////////////////////////////////////////////
// Handshake Message Futures

/// A future for receiving a Noise handshake message.
async fn recv<T>(state: &mut State<T>) -> Result<Bytes, Error>
where
    T: AsyncRead + Unpin,
{
    match state.io.next().await {
        None => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "eof").into()),
        Some(Err(e)) => Err(e.into()),
        Some(Ok(m)) => Ok(m),
    }
}

/// A future for receiving a Noise handshake message with an empty payload.
pub(crate) async fn recv_empty<T>(state: &mut State<T>) -> Result<(), Error>
where
    T: AsyncRead + Unpin,
{
    let msg = recv(state).await?;
    if !msg.is_empty() {
        return Err(
            io::Error::new(io::ErrorKind::InvalidData, "Unexpected handshake payload.").into(),
        );
    }
    Ok(())
}

/// A future for sending a Noise handshake message with an empty payload.
pub(crate) async fn send_empty<T>(state: &mut State<T>) -> Result<(), Error>
where
    T: AsyncWrite + Unpin,
{
    state.io.send(&Vec::new()).await?;
    Ok(())
}

/// A future for receiving a Noise handshake message with a payload identifying the remote.
pub(crate) async fn recv_identity<T>(state: &mut State<T>) -> Result<(), Error>
where
    T: AsyncRead + Unpin,
{
    let msg = recv(state).await?;
    let mut reader = BytesReader::from_bytes(&msg[..]);
    let pb =
        proto::NoiseHandshakePayload::from_reader(&mut reader, &msg[..]).map_err(DecodeError)?;

    state.id_remote_pubkey = Some(identity::PublicKey::try_decode_protobuf(&pb.identity_key)?);

    if !pb.identity_sig.is_empty() {
        state.dh_remote_pubkey_sig = Some(pb.identity_sig);
    }

    Ok(())
}

/// Send a Noise handshake message with a payload identifying the local node to the remote.
pub(crate) async fn send_identity<T>(state: &mut State<T>) -> Result<(), Error>
where
    T: AsyncWrite + Unpin,
{
    let mut pb = proto::NoiseHandshakePayload {
        identity_key: state.identity.public.encode_protobuf(),
        ..Default::default()
    };

    pb.identity_sig = state.identity.signature.clone();

    let mut msg = Vec::with_capacity(pb.get_size());

    let mut writer = Writer::new(&mut msg);
    pb.write_message(&mut writer).expect("Encoding to succeed");
    state.io.send(&msg).await?;

    Ok(())
}
