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

#[allow(clippy::derive_partial_eq_without_eq)]
mod payload_proto {
    include!(concat!(env!("OUT_DIR"), "/payload.proto.rs"));
}

use crate::io::{framed::NoiseFramed, NoiseOutput};
use crate::protocol::{KeypairIdentity, Protocol, PublicKey};
use crate::LegacyConfig;
use crate::NoiseError;
use bytes::Bytes;
use futures::prelude::*;
use libp2p_core::identity;
use prost::Message;
use std::io;

/// The identity of the remote established during a handshake.
pub enum RemoteIdentity<C> {
    /// The remote provided no identifying information.
    ///
    /// The identity of the remote is unknown and must be obtained through
    /// a different, out-of-band channel.
    Unknown,

    /// The remote provided a static DH public key.
    ///
    /// The static DH public key is authentic in the sense that a successful
    /// handshake implies that the remote possesses a corresponding secret key.
    ///
    /// > **Note**: To rule out active attacks like a MITM, trust in the public key must
    /// > still be established, e.g. by comparing the key against an expected or
    /// > otherwise known public key.
    StaticDhKey(PublicKey<C>),

    /// The remote provided a public identity key in addition to a static DH
    /// public key and the latter is authentic w.r.t. the former.
    ///
    /// > **Note**: To rule out active attacks like a MITM, trust in the public key must
    /// > still be established, e.g. by comparing the key against an expected or
    /// > otherwise known public key.
    IdentityKey(identity::PublicKey),
}

//////////////////////////////////////////////////////////////////////////////
// Internal

/// Handshake state.
pub struct State<T> {
    /// The underlying I/O resource.
    io: NoiseFramed<T, snow::HandshakeState>,
    /// The associated public identity of the local node's static DH keypair,
    /// which can be sent to the remote as part of an authenticated handshake.
    identity: KeypairIdentity,
    /// The received signature over the remote's static DH public key, if any.
    dh_remote_pubkey_sig: Option<Vec<u8>>,
    /// The known or received public identity key of the remote, if any.
    id_remote_pubkey: Option<identity::PublicKey>,
    /// Legacy configuration parameters.
    legacy: LegacyConfig,
}

impl<T> State<T> {
    /// Initializes the state for a new Noise handshake, using the given local
    /// identity keypair and local DH static public key. The handshake messages
    /// will be sent and received on the given I/O resource and using the
    /// provided session for cryptographic operations according to the chosen
    /// Noise handshake pattern.
    pub fn new(
        io: T,
        session: snow::HandshakeState,
        identity: KeypairIdentity,
        expected_remote_key: Option<identity::PublicKey>,
        legacy: LegacyConfig,
    ) -> Self {
        Self {
            identity,
            io: NoiseFramed::new(io, session),
            dh_remote_pubkey_sig: None,
            id_remote_pubkey: expected_remote_key,
            legacy,
        }
    }
}

impl<T> State<T> {
    /// Finish a handshake, yielding the established remote identity and the
    /// [`NoiseOutput`] for communicating on the encrypted channel.
    pub fn finish<C>(self) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
    where
        C: Protocol<C> + AsRef<[u8]>,
    {
        let (pubkey, io) = self.io.into_transport()?;
        let remote = match (self.id_remote_pubkey, pubkey) {
            (_, None) => RemoteIdentity::Unknown,
            (None, Some(dh_pk)) => RemoteIdentity::StaticDhKey(dh_pk),
            (Some(id_pk), Some(dh_pk)) => {
                if C::verify(&id_pk, &dh_pk, &self.dh_remote_pubkey_sig) {
                    RemoteIdentity::IdentityKey(id_pk)
                } else {
                    return Err(NoiseError::BadSignature);
                }
            }
        };
        Ok((remote, io))
    }
}

//////////////////////////////////////////////////////////////////////////////
// Handshake Message Futures

/// A future for receiving a Noise handshake message.
async fn recv<T>(state: &mut State<T>) -> Result<Bytes, NoiseError>
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
pub async fn recv_empty<T>(state: &mut State<T>) -> Result<(), NoiseError>
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
pub async fn send_empty<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: AsyncWrite + Unpin,
{
    state.io.send(&Vec::new()).await?;
    Ok(())
}

/// A future for receiving a Noise handshake message with a payload
/// identifying the remote.
///
/// In case `expected_key` is passed, this function will fail if the received key does not match the expected key.
/// In case the remote does not send us a key, the expected key is assumed to be the remote's key.
pub async fn recv_identity<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: AsyncRead + Unpin,
{
    let msg = recv(state).await?;

    let mut pb_result = payload_proto::NoiseHandshakePayload::decode(&msg[..]);

    if pb_result.is_err() && state.legacy.recv_legacy_handshake {
        // NOTE: This is support for legacy handshake payloads. As long as
        // the frame length is less than 256 bytes, which is the case for
        // all protobuf payloads not containing RSA keys, there is no room
        // for misinterpretation, since if a two-bytes length prefix is present
        // the first byte will be 0, which is always an unexpected protobuf tag
        // value because the fields in the .proto file start with 1 and decoding
        // thus expects a non-zero first byte. We will therefore always correctly
        // fall back to the legacy protobuf parsing in these cases (again, not
        // considering RSA keys, for which there may be a probabilistically
        // very small chance of misinterpretation).
        pb_result = pb_result.or_else(|e| {
            if msg.len() > 2 {
                let mut buf = [0, 0];
                buf.copy_from_slice(&msg[..2]);
                // If there is a second length it must be 2 bytes shorter than the
                // frame length, because each length is encoded as a `u16`.
                if usize::from(u16::from_be_bytes(buf)) + 2 == msg.len() {
                    log::debug!("Attempting fallback legacy protobuf decoding.");
                    payload_proto::NoiseHandshakePayload::decode(&msg[2..])
                } else {
                    Err(e)
                }
            } else {
                Err(e)
            }
        });
    }
    let pb = pb_result?;

    if !pb.identity_key.is_empty() {
        let pk = identity::PublicKey::from_protobuf_encoding(&pb.identity_key)?;
        if let Some(ref k) = state.id_remote_pubkey {
            if k != &pk {
                return Err(NoiseError::UnexpectedKey);
            }
        }
        state.id_remote_pubkey = Some(pk);
    }

    if !pb.identity_sig.is_empty() {
        state.dh_remote_pubkey_sig = Some(pb.identity_sig);
    }

    Ok(())
}

/// Send a Noise handshake message with a payload identifying the local node to the remote.
pub async fn send_identity<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: AsyncWrite + Unpin,
{
    let mut pb = payload_proto::NoiseHandshakePayload {
        identity_key: state.identity.public.to_protobuf_encoding(),
        ..Default::default()
    };

    if let Some(ref sig) = state.identity.signature {
        pb.identity_sig = sig.clone()
    }

    let mut msg = if state.legacy.send_legacy_handshake {
        let mut msg = Vec::with_capacity(2 + pb.encoded_len());
        msg.extend_from_slice(&(pb.encoded_len() as u16).to_be_bytes());
        msg
    } else {
        Vec::with_capacity(pb.encoded_len())
    };

    pb.encode(&mut msg)
        .expect("Vec<u8> provides capacity as needed");
    state.io.send(&msg).await?;

    Ok(())
}

/// Send a Noise handshake message with a payload identifying the local node to the remote.
pub async fn send_signature_only<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: AsyncWrite + Unpin,
{
    let mut pb = payload_proto::NoiseHandshakePayload::default();

    if let Some(ref sig) = state.identity.signature {
        pb.identity_sig = sig.clone()
    }

    let mut msg = if state.legacy.send_legacy_handshake {
        let mut msg = Vec::with_capacity(2 + pb.encoded_len());
        msg.extend_from_slice(&(pb.encoded_len() as u16).to_be_bytes());
        msg
    } else {
        Vec::with_capacity(pb.encoded_len())
    };

    pb.encode(&mut msg)
        .expect("Vec<u8> provides capacity as needed");
    state.io.send(&msg).await?;

    Ok(())
}
