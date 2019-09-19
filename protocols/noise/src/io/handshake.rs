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

mod payload;

use crate::error::NoiseError;
use crate::protocol::{Protocol, PublicKey, KeypairIdentity};
use libp2p_core::identity;
use futures::prelude::*;
use futures::task;
use futures::io::AsyncReadExt;
use protobuf::Message;
use std::pin::Pin;
use super::NoiseOutput;

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
    IdentityKey(identity::PublicKey)
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
    None { remote: identity::PublicKey }
}

/// A future performing a Noise handshake pattern.
pub struct Handshake<T, C>(
    Pin<Box<dyn Future<
        Output = <Handshake<T, C> as Future>::Output
    > + Send>>
);

impl<T, C> Future for Handshake<T, C> {
    type Output = Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>;

    fn poll(mut self: Pin<&mut Self>, ctx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
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
    session: Result<snow::Session, NoiseError>,
    identity: KeypairIdentity,
    identity_x: IdentityExchange
) -> Handshake<T, C>
where
    T: AsyncWrite + AsyncRead + Send + Unpin + 'static,
    C: Protocol<C> + AsRef<[u8]>
{
    Handshake(Box::pin(async move {
        let mut state = State::new(io, session, identity, identity_x)?;
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
    session: Result<snow::Session, NoiseError>,
    identity: KeypairIdentity,
    identity_x: IdentityExchange,
) -> Handshake<T, C>
where
    T: AsyncWrite + AsyncRead + Send + Unpin + 'static,
    C: Protocol<C> + AsRef<[u8]>
{
    Handshake(Box::pin(async move {
        let mut state = State::new(io, session, identity, identity_x)?;
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
    session: Result<snow::Session, NoiseError>,
    identity: KeypairIdentity,
    identity_x: IdentityExchange
) -> Handshake<T, C>
where
    T: AsyncWrite + AsyncRead + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]>
{
    Handshake(Box::pin(async move {
        let mut state = State::new(io, session, identity, identity_x)?;
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
    session: Result<snow::Session, NoiseError>,
    identity: KeypairIdentity,
    identity_x: IdentityExchange
) -> Handshake<T, C>
where
    T: AsyncWrite + AsyncRead + Unpin + Send + 'static,
    C: Protocol<C> + AsRef<[u8]>
{
    Handshake(Box::pin(async move {
        let mut state = State::new(io, session, identity, identity_x)?;
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
    io: NoiseOutput<T>,
    /// The associated public identity of the local node's static DH keypair,
    /// which can be sent to the remote as part of an authenticated handshake.
    identity: KeypairIdentity,
    /// The received signature over the remote's static DH public key, if any.
    dh_remote_pubkey_sig: Option<Vec<u8>>,
    /// The known or received public identity key of the remote, if any.
    id_remote_pubkey: Option<identity::PublicKey>,
    /// Whether to send the public identity key of the local node to the remote.
    send_identity: bool,
}

impl<T> State<T> {
    /// Initializes the state for a new Noise handshake, using the given local
    /// identity keypair and local DH static public key. The handshake messages
    /// will be sent and received on the given I/O resource and using the
    /// provided session for cryptographic operations according to the chosen
    /// Noise handshake pattern.
    fn new(
        io: T,
        session: Result<snow::Session, NoiseError>,
        identity: KeypairIdentity,
        identity_x: IdentityExchange
    ) -> Result<Self, NoiseError> {
        let (id_remote_pubkey, send_identity) = match identity_x {
            IdentityExchange::Mutual => (None, true),
            IdentityExchange::Send { remote } => (Some(remote), true),
            IdentityExchange::Receive => (None, false),
            IdentityExchange::None { remote } => (Some(remote), false)
        };
        session.map(|s|
            State {
                identity,
                io: NoiseOutput::new(io, s),
                dh_remote_pubkey_sig: None,
                id_remote_pubkey,
                send_identity
            }
        )
    }
}

impl<T> State<T>
{
    /// Finish a handshake, yielding the established remote identity and the
    /// [`NoiseOutput`] for communicating on the encrypted channel.
    fn finish<C>(self) -> Result<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
    where
        C: Protocol<C> + AsRef<[u8]>
    {
        let dh_remote_pubkey = match self.io.session.get_remote_static() {
            None => None,
            Some(k) => match C::public_from_bytes(k) {
                Err(e) => return Err(e),
                Ok(dh_pk) => Some(dh_pk)
            }
        };
        match self.io.session.into_transport_mode() {
            Err(e) => Err(e.into()),
            Ok(s) => {
                let remote = match (self.id_remote_pubkey, dh_remote_pubkey) {
                    (_, None) => RemoteIdentity::Unknown,
                    (None, Some(dh_pk)) => RemoteIdentity::StaticDhKey(dh_pk),
                    (Some(id_pk), Some(dh_pk)) => {
                        if C::verify(&id_pk, &dh_pk, &self.dh_remote_pubkey_sig) {
                            RemoteIdentity::IdentityKey(id_pk)
                        } else {
                            return Err(NoiseError::InvalidKey)
                        }
                    }
                };
                Ok((remote, NoiseOutput { session: s, .. self.io }))
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Handshake Message Futures

/// A future for receiving a Noise handshake message with an empty payload.
async fn recv_empty<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: AsyncRead + Unpin
{
    state.io.read(&mut []).await?;
    Ok(())
}

/// A future for sending a Noise handshake message with an empty payload.
async fn send_empty<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: AsyncWrite + Unpin
{
    state.io.write(&[]).await?;
    state.io.flush().await?;
    Ok(())
}

/// A future for receiving a Noise handshake message with a payload
/// identifying the remote.
async fn recv_identity<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: AsyncRead + Unpin,
{
    let mut len_buf = [0,0];
    state.io.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;

    let mut payload_buf = vec![0; len];
    state.io.read_exact(&mut payload_buf).await?;
    let pb: payload::Identity = protobuf::parse_from_bytes(&payload_buf)?;

    if !pb.pubkey.is_empty() {
        let pk = identity::PublicKey::from_protobuf_encoding(pb.get_pubkey())
            .map_err(|_| NoiseError::InvalidKey)?;
        if let Some(ref k) = state.id_remote_pubkey {
            if k != &pk {
                return Err(NoiseError::InvalidKey)
            }
        }
        state.id_remote_pubkey = Some(pk);
    }
    if !pb.signature.is_empty() {
        state.dh_remote_pubkey_sig = Some(pb.signature);
    }

    Ok(())
}

/// Send a Noise handshake message with a payload identifying the local node to the remote.
async fn send_identity<T>(state: &mut State<T>) -> Result<(), NoiseError>
where
    T: AsyncWrite + Unpin,
{
    let mut pb = payload::Identity::new();
    if state.send_identity {
        pb.set_pubkey(state.identity.public.clone().into_protobuf_encoding());
    }
    if let Some(ref sig) = state.identity.signature {
        pb.set_signature(sig.clone());
    }
    let pb_bytes = pb.write_to_bytes()?;
    let len = (pb_bytes.len() as u16).to_be_bytes();
    state.io.write_all(&len).await?;
    state.io.write_all(&pb_bytes).await?;
    state.io.flush().await?;
    Ok(())
}
