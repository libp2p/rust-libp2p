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
use crate::protocol::{Protocol, PublicKey};
use libp2p_core::identity;
use futures::{future, Async, Future, future::FutureResult, Poll};
use std::{mem, io};
use tokio_io::{io as nio, AsyncWrite, AsyncRead};
use protobuf::Message;

use super::NoiseOutput;

/// A future performing a Noise handshake pattern.
///
/// In the context of libp2p, an authenticated handshake is a handshake establishing
/// the authenticity of a remote's [public identity key](identity::PublicKey).
///
/// All authenticated handshakes are currently based in turn on authenticated
/// Noise handshakes, i.e. involving static DH keys, in order to allow one or
/// both parties to omit signatures in a handshake if their public identity key is
/// [linked to](Protocol::linked) (e.g. is the same as) their static DH public key.
///
/// An authenticated handshake _succeeds_ if the future resolves with a
/// [`RemoteIdentity::IdentityKey`].
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

    /// The remote provided a public identity key.
    ///
    /// The public identity key is authentic in the sense that a successful handshake
    /// implies that the remote possesses a corresponding secret key.
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
/// > handshake will still create and send signatures over the static DH
/// > public keys.
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
        id_keys: identity::Keypair,
        dh_pubkey: PublicKey<C>,
        io: T,
        session: Result<snow::Session, NoiseError>,
        id_exchange: IdentityExchange
    ) -> Handshake<T, C> {
        Handshake(Box::new(
            State::new(id_keys, dh_pubkey, io, session, id_exchange)
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
        id_keys: identity::Keypair,
        dh_pubkey: PublicKey<C>,
        io: T,
        session: Result<snow::Session, NoiseError>,
        id_exchange: IdentityExchange,
    ) -> Handshake<T, C> {
        Handshake(Box::new(
            State::new(id_keys, dh_pubkey, io, session, id_exchange)
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
        id_keys: identity::Keypair,
        dh_pubkey: PublicKey<C>,
        io: T,
        session: Result<snow::Session, NoiseError>,
        id_exchange: IdentityExchange
    ) -> Handshake<T, C> {
        Handshake(Box::new(
            State::new(id_keys, dh_pubkey, io, session, id_exchange)
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
        id_keys: identity::Keypair,
        dh_pubkey: PublicKey<C>,
        io: T,
        session: Result<snow::Session, NoiseError>,
        id_exchange: IdentityExchange
    ) -> Handshake<T, C> {
        Handshake(Box::new(
            State::new(id_keys, dh_pubkey, io, session, id_exchange)
                .and_then(State::recv_empty)
                .and_then(State::send_identity)
                .and_then(State::recv_identity)
                .and_then(State::finish)))
    }
}

//////////////////////////////////////////////////////////////////////////////
// Internal

/// Handshake state.
struct State<T, C> {
    /// The underlying I/O resource.
    io: NoiseOutput<T>,
    /// The local node's identity keypair.
    id_keys: identity::Keypair,
    /// The local node's static DH public key.
    dh_pubkey: PublicKey<C>,
    /// The received signature over the remote's static DH public key, if any.
    dh_remote_pubkey_sig: Option<Vec<u8>>,
    /// The known or received public identity key of the remote, if any.
    id_remote_pubkey: Option<identity::PublicKey>,
    /// Whether to send the public identity key of the local node to the remote.
    send_id: bool,
}

impl<T: io::Read, C> io::Read for State<T, C> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.io.read(buf)
    }
}

impl<T: io::Write, C> io::Write for State<T, C> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.io.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.io.flush()
    }
}

impl<T: AsyncRead, C> AsyncRead for State<T, C> {}

impl<T: AsyncWrite, C> AsyncWrite for State<T, C> {
    fn shutdown(&mut self) -> Poll<(), io::Error> {
        self.io.shutdown()
    }
}

impl<T, C> State<T, C> {
    /// Initializes the state for a new Noise handshake, using the given local
    /// identity keypair and local DH static public key. The handshake messages
    /// will be sent and received on the given I/O resource and using the
    /// provided session for cryptographic operations according to the chosen
    /// Noise handshake pattern.
    fn new(
        id: identity::Keypair,
        dh: PublicKey<C>,
        io: T,
        ss: Result<snow::Session, NoiseError>,
        id_exchange: IdentityExchange
    ) -> FutureResult<Self, NoiseError> {
        let (id_remote_pubkey, send_id) = match id_exchange {
            IdentityExchange::Mutual => (None, true),
            IdentityExchange::Send { remote } => (Some(remote), true),
            IdentityExchange::Receive => (None, false),
            IdentityExchange::None { remote } => (Some(remote), false)
        };
        future::result(ss.map(|s|
            State {
                id_keys: id,
                dh_pubkey: dh,
                io: NoiseOutput::new(io, s),
                dh_remote_pubkey_sig: None,
                id_remote_pubkey,
                send_id
            }
        ))
    }
}

impl<T, C> State<T, C>
where
    C: Protocol<C> + AsRef<[u8]>
{
    /// Finish a handshake, yielding the established remote identity and the
    /// [`NoiseOutput`] for communicating on the encrypted channel.
    fn finish(self) -> FutureResult<(RemoteIdentity<C>, NoiseOutput<T>), NoiseError> {
        let dh_remote_pubkey = match self.io.session.get_remote_static() {
            None => None,
            Some(k) => match C::public_from_bytes(k) {
                Err(e) => return future::result(Err(e)),
                Ok(dh_pk) => Some(dh_pk)
            }
        };
        future::result(match self.io.session.into_transport_mode() {
            Err(e) => Err(e.into()),
            Ok(s) => {
                let remote = match (self.id_remote_pubkey, dh_remote_pubkey) {
                    (_, None) => RemoteIdentity::Unknown,
                    (None, Some(dh_pk)) => RemoteIdentity::StaticDhKey(dh_pk),
                    (Some(id_pk), Some(dh_pk)) => {
                        if C::verify(&id_pk, &dh_pk, &self.dh_remote_pubkey_sig) {
                            RemoteIdentity::IdentityKey(id_pk)
                        } else {
                            return future::result(Err(NoiseError::InvalidKey))
                        }
                    }
                };
                Ok((remote, NoiseOutput { session: s, .. self.io }))
            }
        })
    }
}

impl<T, C> State<T, C> {
    /// Creates a future that sends a Noise handshake message with an empty payload.
    fn send_empty(self) -> SendEmpty<T, C> {
        SendEmpty { state: SendState::Write(self) }
    }

    /// Creates a future that expects to receive a Noise handshake message with an empty payload.
    fn recv_empty(self) -> RecvEmpty<T, C> {
        RecvEmpty { state: RecvState::Read(self) }
    }

    /// Creates a future that sends a Noise handshake message with a payload identifying
    /// the local node to the remote.
    fn send_identity(self) -> SendIdentity<T, C> {
        SendIdentity { state: SendIdentityState::Init(self) }
    }

    /// Creates a future that expects to receive a Noise handshake message with a
    /// payload identifying the remote.
    fn recv_identity(self) -> RecvIdentity<T, C> {
        RecvIdentity { state: RecvIdentityState::Init(self) }
    }
}

//////////////////////////////////////////////////////////////////////////////
// Handshake Message Futures

// RecvEmpty -----------------------------------------------------------------

/// A future for receiving a Noise handshake message with an empty payload.
///
/// Obtained from [`Handshake::recv_empty`].
struct RecvEmpty<T, C> {
    state: RecvState<T, C>
}

enum RecvState<T, C> {
    Read(State<T, C>),
    Done
}

impl<T, C> Future for RecvEmpty<T, C>
where
    T: AsyncRead
{
    type Error = NoiseError;
    type Item = State<T, C>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use RecvState::*;
        match mem::replace(&mut self.state, Done) {
            Read(mut st) =>  {
                if !st.io.poll_read(&mut [])?.is_ready() {
                    self.state = Read(st);
                    return Ok(Async::NotReady)
                }
                Ok(Async::Ready(st))
            },
            Done => panic!("RecvEmpty polled after completion")
        }
    }
}

// SendEmpty -----------------------------------------------------------------

/// A future for sending a Noise handshake message with an empty payload.
///
/// Obtained from [`Handshake::send_empty`].
struct SendEmpty<T, C> {
    state: SendState<T, C>
}

enum SendState<T, C> {
    Write(State<T, C>),
    Flush(State<T, C>),
    Done
}

impl<T, C> Future for SendEmpty<T, C>
where
    T: AsyncWrite
{
    type Error = NoiseError;
    type Item = State<T, C>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use SendState::*;
        loop {
            match mem::replace(&mut self.state, Done) {
                Write(mut st) => {
                    if !st.io.poll_write(&mut [])?.is_ready() {
                        self.state = Write(st);
                        return Ok(Async::NotReady)
                    }
                    self.state = Flush(st);
                },
                Flush(mut st) => {
                    if !st.io.poll_flush()?.is_ready() {
                        self.state = Flush(st);
                        return Ok(Async::NotReady)
                    }
                    return Ok(Async::Ready(st))
                }
                Done => panic!("SendEmpty polled after completion")
            }
        }
    }
}

// RecvIdentity --------------------------------------------------------------

/// A future for receiving a Noise handshake message with a payload
/// identifying the remote.
///
/// Obtained from [`Handshake::recv_identity`].
struct RecvIdentity<T, C> {
    state: RecvIdentityState<T, C>
}

enum RecvIdentityState<T, C> {
    Init(State<T, C>),
    ReadPayloadLen(nio::ReadExact<State<T, C>, [u8; 2]>),
    ReadPayload(nio::ReadExact<State<T, C>, Vec<u8>>),
    Done
}

impl<T, C> Future for RecvIdentity<T, C>
where
    T: AsyncRead,
{
    type Error = NoiseError;
    type Item = State<T, C>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use RecvIdentityState::*;
        loop {
            match mem::replace(&mut self.state, Done) {
                Init(st) => {
                    self.state = ReadPayloadLen(nio::read_exact(st, [0u8; 2]));
                },
                ReadPayloadLen(mut read_len) => {
                    if let Async::Ready((st, bytes)) = read_len.poll()? {
                        let mut len_be = [0u8; 2];
                        len_be.copy_from_slice(&bytes);
                        let len = u16::from_be_bytes(len_be) as usize;
                        self.state = ReadPayload(nio::read_exact(st, vec![0; len as usize]));
                    } else {
                        self.state = ReadPayloadLen(read_len);
                        return Ok(Async::NotReady);
                    }
                },
                ReadPayload(mut read_payload) => {
                    if let Async::Ready((mut st, bytes)) = read_payload.poll()? {
                        let pb: payload::Identity = protobuf::parse_from_bytes(&bytes)?;
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
                        self.state = ReadPayload(read_payload);
                        return Ok(Async::NotReady)
                    }
                },
                Done => panic!("RecvIdentity polled after completion")
            }
        }
    }
}

// SendIdentity --------------------------------------------------------------

/// A future for sending a Noise handshake message with a payload
/// identifying the local node to the remote.
///
/// Obtained from [`Handshake::send_identity`].
struct SendIdentity<T, C> {
    state: SendIdentityState<T, C>
}

enum SendIdentityState<T, C> {
    Init(State<T, C>),
    WritePayloadLen(nio::WriteAll<State<T, C>, [u8; 2]>, Vec<u8>),
    WritePayload(nio::WriteAll<State<T, C>, Vec<u8>>),
    Flush(State<T, C>),
    Done
}

impl<T, C> Future for SendIdentity<T, C>
where
    T: AsyncWrite,
    C: Protocol<C> + AsRef<[u8]>
{
    type Error = NoiseError;
    type Item = State<T, C>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use SendIdentityState::*;
        loop {
            match mem::replace(&mut self.state, Done) {
                Init(st) => {
                    let sig = C::sign(&st.id_keys, &st.dh_pubkey)?;
                    let mut pb = payload::Identity::new();
                    if st.send_id {
                        let pk = st.id_keys.public();
                        pb.set_pubkey(pk.into_protobuf_encoding());
                    }
                    if let Some(sig) = sig {
                        pb.set_signature(sig);
                    }
                    let pb_bytes = pb.write_to_bytes()?;
                    let len = (pb_bytes.len() as u16).to_be_bytes();
                    self.state = WritePayloadLen(nio::write_all(st, len), pb_bytes);
                },
                WritePayloadLen(mut write_len, payload) => {
                    if let Async::Ready((st, _)) = write_len.poll()? {
                        self.state = WritePayload(nio::write_all(st, payload));
                    } else {
                        self.state = WritePayloadLen(write_len, payload);
                        return Ok(Async::NotReady)
                    }
                },
                WritePayload(mut write_payload) => {
                    if let Async::Ready((st, _)) = write_payload.poll()? {
                        self.state = Flush(st);
                    } else {
                        self.state = WritePayload(write_payload);
                        return Ok(Async::NotReady)
                    }
                },
                Flush(mut st) => {
                    if !st.poll_flush()?.is_ready() {
                        self.state = Flush(st);
                        return Ok(Async::NotReady)
                    }
                    return Ok(Async::Ready(st))
                },
                Done => panic!("SendIdentity polled after completion")
            }
        }
    }
}

