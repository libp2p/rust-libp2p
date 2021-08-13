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

mod payload_proto {
    include!(concat!(env!("OUT_DIR"), "/payload.proto.rs"));
}

use crate::error::NoiseError;
use crate::io::{framed::NoiseFramed, NoiseOutput};
use crate::protocol::{KeypairIdentity, Protocol, PublicKey};
use crate::LegacyConfig;
use bytes::Bytes;
use futures::prelude::*;
use futures::task;
use libp2p_core::identity;
use prost::Message;
use std::{io, pin::Pin, task::Context};

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

/// The options for identity exchange in an authenticated handshake.
///
/// > **Note**: Even if a remote's public identity key is known a priori,
/// > unless the authenticity of the key is [linked](Protocol::linked) to
/// > the authenticity of a remote's static DH public key, an authenticated
/// > handshake will still send the associated signature of the provided
/// > local [`KeypairIdentity`] in order for the remote to verify that the static
/// > DH public key is authentic w.r.t. the known public identity key.
pub enum IdentityExchange {
    /// Send the local public identity to the remote.
    ///
    /// The remote identity is unknown (i.e. expected to be received).
    Mutual,
    /// Send the local public identity to the remote.
    ///
    /// The remote identity is known.
    Send { remote: identity::PublicKey },
    /// Don't send the local public identity to the remote.
    ///
    /// The remote identity is unknown, i.e. expected to be received.
    Receive,
    /// Don't send the local public identity to the remote.
    ///
    /// The remote identity is known, thus identities must be mutually known
    /// in order for the handshake to succeed.
    None { remote: identity::PublicKey },
}

/// A future performing a Noise handshake pattern.
pub struct Handshake<T, C>(
    Pin<Box<dyn Future<Output = Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>> + Send>>,
);

impl<T, C> Future for Handshake<T, C> {
    type Output = Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context<'_>) -> task::Poll<Self::Output> {
        Pin::new(&mut self.0).poll(ctx)
    }
}

/// Creates an authenticated Noise handshake for the initiator of a
/// single roundtrip (2 message) handshake pattern.
///
/// Subject to the chosen [`IdentityExchange`], this message sequence
/// identifies the local node to the remote with the first message payload
/// (i.e. unencrypted) and expects the remote to identify itself in the
/// second message payload.
///
/// This message sequence is suitable for authenticated 2-message Noise handshake
/// patterns where the static keys of the initiator and responder are either
/// known (i.e. appear in the pre-message pattern) or are sent with
/// the first and second message, respectively (e.g. `IK` or `IX`).
///
/// ```raw
/// initiator -{id}-> responder
/// initiator <-{id}- responder
/// ```
pub fn rt1_initiator<T, C>(
    io: T,
    session: Result<snow::HandshakeState, NoiseError>,
    identity: KeypairIdentity,
    identity_x: IdentityExchange,
    legacy: LegacyConfig,
) -> Handshake<T, C>
where
    T: AsyncWrite + AsyncRead + Send + Unpin + 'static,
    C: Protocol<C> + AsRef<[u8]>,
{
    Handshake(Box::pin(async move {
        let mut state = State::new(io, session, identity, identity_x, legacy)?;
        send_identity(&mut state).await?;
        recv_identity(&mut state).await?;
        state.finish()
    }))
}

/// Creates an authenticated Noise handshake for the responder of a
/// single roundtrip (2 message) handshake pattern.
///
/// Subject to the chosen [`IdentityExchange`], this message sequence expects the
/// remote to identify itself in the first message payload (i.e. unencrypted)
/// and identifies the local node to the remote in the second message payload.
///
/// This message sequence is suitable for authenticated 2-message Noise handshake
/// patterns where the static keys of the initiator and responder are either
/// known (i.e. appear in the pre-message pattern) or are sent with the first
/// and second message, respectively (e.g. `IK` or `IX`).
///
/// ```raw
/// initiator -{id}-> responder
/// initiator <-{id}- responder
/// ```
pub fn rt1_responder<T, C>(
    io: T,
    session: Result<snow::HandshakeState, NoiseError>,
    identity: KeypairIdentity,
    identity_x: IdentityExchange,
    legacy: LegacyConfig,
) -> Handshake<T, C>
where
    T: AsyncWrite + AsyncRead + Send + Unpin + 'static,
    C: Protocol<C> + AsRef<[u8]>,
{
    Handshake(Box::pin(async move {
        let mut state = State::new(io, session, identity, identity_x, legacy)?;
        recv_identity(&mut state).await?;
        send_identity(&mut state).await?;
        state.finish()
    }))
}

/// Creates an authenticated Noise handshake for the initiator of a
/// 1.5-roundtrip (3 message) handshake pattern.
///
/// Subject to the chosen [`IdentityExchange`], this message sequence expects
/// the remote to identify itself in the second message payload and
/// identifies the local node to the remote in the third message payload.
/// The first (unencrypted) message payload is always empty.
///
/// This message sequence is suitable for authenticated 3-message Noise handshake
/// patterns where the static keys of the responder and initiator are either known
/// (i.e. appear in the pre-message pattern) or are sent with the second and third
/// message, respectively (e.g. `XX`).
///
/// ```raw
/// initiator --{}--> responder
/// initiator <-{id}- responder
/// initiator -{id}-> responder
/// ```
pub fn rt15_initiator<T, C>(
    io: T,
    session: Result<snow::HandshakeState, NoiseError>,
    identity: KeypairIdentity,
    identity_x: IdentityExchange,
    legacy: LegacyConfig,
) -> Handshake<T, C>
where
    T: AsyncWrite + AsyncRead + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]>,
{
    Handshake(Box::pin(async move {
        let mut state = State::new(io, session, identity, identity_x, legacy)?;
        send_empty(&mut state).await?;
        recv_identity(&mut state).await?;
        send_identity(&mut state).await?;
        state.finish()
    }))
}

/// Creates an authenticated Noise handshake for the responder of a
/// 1.5-roundtrip (3 message) handshake pattern.
///
/// Subject to the chosen [`IdentityExchange`], this message sequence
/// identifies the local node in the second message payload and expects
/// the remote to identify itself in the third message payload. The first
/// (unencrypted) message payload is always empty.
///
/// This message sequence is suitable for authenticated 3-message Noise handshake
/// patterns where the static keys of the responder and initiator are either known
/// (i.e. appear in the pre-message pattern) or are sent with the second and third
/// message, respectively (e.g. `XX`).
///
/// ```raw
/// initiator --{}--> responder
/// initiator <-{id}- responder
/// initiator -{id}-> responder
/// ```
pub fn rt15_responder<T, C>(
    io: T,
    session: Result<snow::HandshakeState, NoiseError>,
    identity: KeypairIdentity,
    identity_x: IdentityExchange,
    legacy: LegacyConfig,
) -> Handshake<T, C>
where
    T: AsyncWrite + AsyncRead + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]>,
{
    Handshake(Box::pin(async move {
        let mut state = State::new(io, session, identity, identity_x, legacy)?;
        recv_empty(&mut state).await?;
        send_identity(&mut state).await?;
        recv_identity(&mut state).await?;
        state.finish()
    }))
}

//////////////////////////////////////////////////////////////////////////////
// Internal

/// Handshake state.
struct State<T> {
    /// The underlying I/O resource.
    io: NoiseFramed<T, snow::HandshakeState>,
    /// The associated public identity of the local node's static DH keypair,
    /// which can be sent to the remote as part of an authenticated handshake.
    identity: KeypairIdentity,
    /// The received signature over the remote's static DH public key, if any.
    dh_remote_pubkey_sig: Option<Vec<u8>>,
    /// The known or received public identity key of the remote, if any.
    id_remote_pubkey: Option<identity::PublicKey>,
    /// Whether to send the public identity key of the local node to the remote.
    send_identity: bool,
    /// Legacy configuration parameters.
    legacy: LegacyConfig,
}

impl<T> State<T> {
    /// Initializes the state for a new Noise handshake, using the given local
    /// identity keypair and local DH static public key. The handshake messages
    /// will be sent and received on the given I/O resource and using the
    /// provided session for cryptographic operations according to the chosen
    /// Noise handshake pattern.
    fn new(
        io: T,
        session: Result<snow::HandshakeState, NoiseError>,
        identity: KeypairIdentity,
        identity_x: IdentityExchange,
        legacy: LegacyConfig,
    ) -> Result<Self, NoiseError> {
        let (id_remote_pubkey, send_identity) = match identity_x {
            IdentityExchange::Mutual => (None, true),
            IdentityExchange::Send { remote } => (Some(remote), true),
            IdentityExchange::Receive => (None, false),
            IdentityExchange::None { remote } => (Some(remote), false),
        };
        session.map(|s| State {
            identity,
            io: NoiseFramed::new(io, s),
            dh_remote_pubkey_sig: None,
            id_remote_pubkey,
            send_identity,
            legacy,
        })
    }
}

impl<T> State<T> {
    /// Finish a handshake, yielding the established remote identity and the
    /// [`NoiseOutput`] for communicating on the encrypted channel.
    fn finish<C>(self) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
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
                    return Err(NoiseError::InvalidKey);
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
async fn recv_empty<T>(state: &mut State<T>) -> Result<(), NoiseError>
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
async fn send_empty<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: AsyncWrite + Unpin,
{
    state.io.send(&Vec::new()).await?;
    Ok(())
}

/// A future for receiving a Noise handshake message with a payload
/// identifying the remote.
async fn recv_identity<T>(state: &mut State<T>) -> Result<(), NoiseError>
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
        let pk = identity::PublicKey::from_protobuf_encoding(&pb.identity_key)
            .map_err(|_| NoiseError::InvalidKey)?;
        if let Some(ref k) = state.id_remote_pubkey {
            if k != &pk {
                return Err(NoiseError::InvalidKey);
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
async fn send_identity<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: AsyncWrite + Unpin,
{
    let mut pb = payload_proto::NoiseHandshakePayload::default();

    if state.send_identity {
        pb.identity_key = state.identity.public.to_protobuf_encoding()
    }

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
