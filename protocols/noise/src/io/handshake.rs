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

mod payload_proto;

use crate::error::NoiseError;
use crate::protocol::{Protocol, PublicKey, KeypairIdentity};
use crate::io::SnowState;
use libp2p_core::identity;
use futures::{future, Async, Future, future::FutureResult, Poll};
use std::{mem, io};
use tokio_io::{io as nio, AsyncWrite, AsyncRead};
use protobuf::Message;

use super::NoiseOutput;

/// A future performing a Noise handshake pattern.
pub struct Handshake<T, C>(
    Box<dyn Future<
        Item = <Handshake<T, C> as Future>::Item,
        Error = <Handshake<T, C> as Future>::Error
    > + Send>
);

impl<T, C> Future for Handshake<T, C> {
    type Error = NoiseError;
    type Item = (RemoteIdentity<C>, NoiseOutput<T>);

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.0.poll()
    }
}

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

impl<T, C> Handshake<T, C>
where
    T: AsyncRead + AsyncWrite + Send + 'static,
    C: Protocol<C> + AsRef<[u8]> + Send + 'static,
{
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
    pub fn rt1_initiator(
        io: T,
        session: Result<snow::HandshakeState, NoiseError>,
        identity: KeypairIdentity,
        identity_x: IdentityExchange
    ) -> Handshake<T, C> {
        Handshake(Box::new(
            State::new(io, session, identity, identity_x)
                .and_then(State::send_identity)
                .and_then(State::recv_identity)
                .and_then(State::finish)))
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
    pub fn rt1_responder(
        io: T,
        session: Result<snow::HandshakeState, NoiseError>,
        identity: KeypairIdentity,
        identity_x: IdentityExchange,
    ) -> Handshake<T, C> {
        Handshake(Box::new(
            State::new(io, session, identity, identity_x)
                .and_then(State::recv_identity)
                .and_then(State::send_identity)
                .and_then(State::finish)))
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
    pub fn rt15_initiator(
        io: T,
        session: Result<snow::HandshakeState, NoiseError>,
        identity: KeypairIdentity,
        identity_x: IdentityExchange
    ) -> Handshake<T, C> {
        Handshake(Box::new(
            State::new(io, session, identity, identity_x)
                .and_then(State::send_empty)
                .and_then(State::recv_identity)
                .and_then(State::send_identity)
                .and_then(State::finish)))
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
    pub fn rt15_responder(
        io: T,
        session: Result<snow::HandshakeState, NoiseError>,
        identity: KeypairIdentity,
        identity_x: IdentityExchange
    ) -> Handshake<T, C> {
        Handshake(Box::new(
            State::new(io, session, identity, identity_x)
                .and_then(State::recv_empty)
                .and_then(State::send_identity)
                .and_then(State::recv_identity)
                .and_then(State::finish)))
    }
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

impl<T: io::Read> io::Read for State<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.io.read(buf)
    }
}

impl<T: io::Write> io::Write for State<T> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.io.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.io.flush()
    }
}

impl<T: AsyncRead> AsyncRead for State<T> {
    unsafe fn prepare_uninitialized_buffer(&self, buf: &mut [u8]) -> bool {
        self.io.prepare_uninitialized_buffer(buf)
    }
    fn read_buf<B: bytes::BufMut>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        self.io.read_buf(buf)
    }
}

impl<T: AsyncWrite> AsyncWrite for State<T> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.io.shutdown()
    }
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
        identity_x: IdentityExchange
    ) -> FutureResult<Self, NoiseError> {
        let (id_remote_pubkey, send_identity) = match identity_x {
            IdentityExchange::Mutual => (None, true),
            IdentityExchange::Send { remote } => (Some(remote), true),
            IdentityExchange::Receive => (None, false),
            IdentityExchange::None { remote } => (Some(remote), false)
        };
        future::result(session.map(|s|
            State {
                identity,
                io: NoiseOutput::new(io, SnowState::Handshake(s)),
                dh_remote_pubkey_sig: None,
                id_remote_pubkey,
                send_identity
            }
        ))
    }
}

impl<T> State<T>
{
    /// Finish a handshake, yielding the established remote identity and the
    /// [`NoiseOutput`] for communicating on the encrypted channel.
    fn finish<C>(self) -> FutureResult<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError>
    where
        C: Protocol<C> + AsRef<[u8]>
    {
        let dh_remote_pubkey = match self.io.session.get_remote_static() {
            None => None,
            Some(k) => match C::public_from_bytes(k) {
                Err(e) => return future::err(e),
                Ok(dh_pk) => Some(dh_pk)
            }
        };
        match self.io.session.into_transport_mode() {
            Err(e) => future::err(e.into()),
            Ok(s) => {
                let remote = match (self.id_remote_pubkey, dh_remote_pubkey) {
                    (_, None) => RemoteIdentity::Unknown,
                    (None, Some(dh_pk)) => RemoteIdentity::StaticDhKey(dh_pk),
                    (Some(id_pk), Some(dh_pk)) => {
                        if C::verify(&id_pk, &dh_pk, &self.dh_remote_pubkey_sig) {
                            RemoteIdentity::IdentityKey(id_pk)
                        } else {
                            return future::err(NoiseError::InvalidKey)
                        }
                    }
                };
                future::ok((remote, NoiseOutput { session: SnowState::Transport(s), .. self.io }))
            }
        }
    }
}

impl<T> State<T> {
    /// Creates a future that sends a Noise handshake message with an empty payload.
    fn send_empty(self) -> SendEmpty<T> {
        SendEmpty { state: SendState::Write(self) }
    }

    /// Creates a future that expects to receive a Noise handshake message with an empty payload.
    fn recv_empty(self) -> RecvEmpty<T> {
        RecvEmpty { state: RecvState::Read(self) }
    }

    /// Creates a future that sends a Noise handshake message with a payload identifying
    /// the local node to the remote.
    fn send_identity(self) -> SendIdentity<T> {
        SendIdentity { state: SendIdentityState::Init(self) }
    }

    /// Creates a future that expects to receive a Noise handshake message with a
    /// payload identifying the remote.
    fn recv_identity(self) -> RecvIdentity<T> {
        RecvIdentity { state: RecvIdentityState::Init(self) }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Handshake Message Futures

// RecvEmpty -----------------------------------------------------------------

/// A future for receiving a Noise handshake message with an empty payload.
///
/// Obtained from [`Handshake::recv_empty`].
struct RecvEmpty<T> {
    state: RecvState<T>
}

enum RecvState<T> {
    Read(State<T>),
    Done
}

impl<T> Future for RecvEmpty<T>
where
    T: AsyncRead
{
    type Error = NoiseError;
    type Item = State<T>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match mem::replace(&mut self.state, RecvState::Done) {
            RecvState::Read(mut st) =>  {
                if !st.io.poll_read(&mut [])?.is_ready() {
                    self.state = RecvState::Read(st);
                    return Ok(Async::NotReady)
                }
                Ok(Async::Ready(st))
            },
            RecvState::Done => panic!("RecvEmpty polled after completion")
        }
    }
}

// SendEmpty -----------------------------------------------------------------

/// A future for sending a Noise handshake message with an empty payload.
///
/// Obtained from [`Handshake::send_empty`].
struct SendEmpty<T> {
    state: SendState<T>
}

enum SendState<T> {
    Write(State<T>),
    Flush(State<T>),
    Done
}

impl<T> Future for SendEmpty<T>
where
    T: AsyncWrite
{
    type Error = NoiseError;
    type Item = State<T>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, SendState::Done) {
                SendState::Write(mut st) => {
                    if !st.io.poll_write(&mut [])?.is_ready() {
                        self.state = SendState::Write(st);
                        return Ok(Async::NotReady)
                    }
                    self.state = SendState::Flush(st);
                },
                SendState::Flush(mut st) => {
                    if !st.io.poll_flush()?.is_ready() {
                        self.state = SendState::Flush(st);
                        return Ok(Async::NotReady)
                    }
                    return Ok(Async::Ready(st))
                }
                SendState::Done => panic!("SendEmpty polled after completion")
            }
        }
    }
}

// RecvIdentity --------------------------------------------------------------

/// A future for receiving a Noise handshake message with a payload
/// identifying the remote.
///
/// Obtained from [`Handshake::recv_identity`].
struct RecvIdentity<T> {
    state: RecvIdentityState<T>
}

enum RecvIdentityState<T> {
    Init(State<T>),
    ReadPayloadLen(nio::ReadExact<State<T>, [u8; 2]>),
    ReadPayload(nio::ReadExact<State<T>, Vec<u8>>),
    Done
}

impl<T> Future for RecvIdentity<T>
where
    T: AsyncRead,
{
    type Error = NoiseError;
    type Item = State<T>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, RecvIdentityState::Done) {
                RecvIdentityState::Init(st) => {
                    self.state = RecvIdentityState::ReadPayloadLen(nio::read_exact(st, [0, 0]));
                },
                RecvIdentityState::ReadPayloadLen(mut read_len) => {
                    if let Async::Ready((st, bytes)) = read_len.poll()? {
                        let len = u16::from_be_bytes(bytes) as usize;
                        let buf = vec![0; len];
                        self.state = RecvIdentityState::ReadPayload(nio::read_exact(st, buf));
                    } else {
                        self.state = RecvIdentityState::ReadPayloadLen(read_len);
                        return Ok(Async::NotReady);
                    }
                },
                RecvIdentityState::ReadPayload(mut read_payload) => {
                    if let Async::Ready((mut st, bytes)) = read_payload.poll()? {
                        let pb: payload_proto::Identity = protobuf::parse_from_bytes(&bytes)?;
                        if !pb.pubkey.is_empty() {
                            let pk = identity::PublicKey::from_protobuf_encoding(pb.get_pubkey())
                                .map_err(|_| NoiseError::InvalidKey)?;
                            if let Some(ref k) = st.id_remote_pubkey {
                                if k != &pk {
                                    return Err(NoiseError::InvalidKey)
                                }
                            }
                            st.id_remote_pubkey = Some(pk);
                        }
                        if !pb.signature.is_empty() {
                            st.dh_remote_pubkey_sig = Some(pb.signature)
                        }
                        return Ok(Async::Ready(st))
                    } else {
                        self.state = RecvIdentityState::ReadPayload(read_payload);
                        return Ok(Async::NotReady)
                    }
                },
                RecvIdentityState::Done => panic!("RecvIdentity polled after completion")
            }
        }
    }
}

// SendIdentity --------------------------------------------------------------

/// A future for sending a Noise handshake message with a payload
/// identifying the local node to the remote.
///
/// Obtained from [`Handshake::send_identity`].
struct SendIdentity<T> {
    state: SendIdentityState<T>
}

enum SendIdentityState<T> {
    Init(State<T>),
    WritePayloadLen(nio::WriteAll<State<T>, [u8; 2]>, Vec<u8>),
    WritePayload(nio::WriteAll<State<T>, Vec<u8>>),
    Flush(State<T>),
    Done
}

impl<T> Future for SendIdentity<T>
where
    T: AsyncWrite,
{
    type Error = NoiseError;
    type Item = State<T>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.state, SendIdentityState::Done) {
                SendIdentityState::Init(st) => {
                    let mut pb = payload_proto::Identity::new();
                    if st.send_identity {
                        pb.set_pubkey(st.identity.public.clone().into_protobuf_encoding());
                    }
                    if let Some(ref sig) = st.identity.signature {
                        pb.set_signature(sig.clone());
                    }
                    let pb_bytes = pb.write_to_bytes()?;
                    let len = (pb_bytes.len() as u16).to_be_bytes();
                    let write_len = nio::write_all(st, len);
                    self.state = SendIdentityState::WritePayloadLen(write_len, pb_bytes);
                },
                SendIdentityState::WritePayloadLen(mut write_len, payload) => {
                    if let Async::Ready((st, _)) = write_len.poll()? {
                        self.state = SendIdentityState::WritePayload(nio::write_all(st, payload));
                    } else {
                        self.state = SendIdentityState::WritePayloadLen(write_len, payload);
                        return Ok(Async::NotReady)
                    }
                },
                SendIdentityState::WritePayload(mut write_payload) => {
                    if let Async::Ready((st, _)) = write_payload.poll()? {
                        self.state = SendIdentityState::Flush(st);
                    } else {
                        self.state = SendIdentityState::WritePayload(write_payload);
                        return Ok(Async::NotReady)
                    }
                },
                SendIdentityState::Flush(mut st) => {
                    if !st.poll_flush()?.is_ready() {
                        self.state = SendIdentityState::Flush(st);
                        return Ok(Async::NotReady)
                    }
                    return Ok(Async::Ready(st))
                },
                SendIdentityState::Done => panic!("SendIdentity polled after completion")
            }
        }
    }
}

