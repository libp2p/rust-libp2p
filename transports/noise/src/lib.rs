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

mod error;
mod io;

pub use error::NoiseError;
pub use io::NoiseOutput;

use futures::{prelude::*, try_ready};
use libp2p_core::{multiaddr::{Multiaddr, Protocol}, PeerId, Transport, TransportError};
use log::{debug, trace};
use rand::Rng;
use snow;
use std::{mem, sync::Arc};
use tokio_io::{AsyncRead, AsyncWrite};
use x25519_dalek::{x25519, X25519_BASEPOINT_BYTES};

const PATTERN: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";

#[derive(Clone, Debug)]
pub struct PublicKey([u8; 32]);

impl PublicKey {
    pub fn base58_encoded(&self) -> String {
        bs58::encode(self.0).into_string()
    }
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Curve25519 keypair.
pub struct Keypair {
    secret: [u8; 32],
    public: PublicKey
}

impl Keypair {
    pub fn fresh() -> Self {
        let mut secret = [0; 32];
        rand::thread_rng().fill(&mut secret);
        let public = x25519(secret, X25519_BASEPOINT_BYTES);
        Keypair { secret, public: PublicKey(public) }
    }

    pub fn secret(&self) -> &[u8; 32] {
        &self.secret
    }

    pub fn public(&self) -> &PublicKey {
        &self.public
    }
}

#[derive(Clone)]
pub struct NoiseConfig<T> {
    keypair: Arc<Keypair>,
    params: snow::params::NoiseParams,
    transport: T
}

impl<T: Transport> NoiseConfig<T> {
    pub fn new(transport: T, kp: Keypair) -> Self {
        NoiseConfig {
            keypair: Arc::new(kp),
            params: PATTERN.parse().expect("constant pattern always parses successfully"),
            transport
        }
    }
}

impl<T> Transport for NoiseConfig<T>
where
    T: Transport,
    T::Output: AsyncRead + AsyncWrite,
    T::Error: 'static
{
    type Output = (PeerId, NoiseOutput<T::Output>);
    type Error = NoiseError<T::Error>;
    type Listener = NoiseListener<T::Listener>;
    type ListenerUpgrade = NoiseListenFuture<T::ListenerUpgrade>;
    type Dial = NoiseDialFuture<T::Dial>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>> {
        let (listener, addr) = self.transport.listen_on(addr)
            .map_err(|e| match e {
                TransportError::MultiaddrNotSupported(a) => TransportError::MultiaddrNotSupported(a),
                TransportError::Other(e) => TransportError::Other(NoiseError::Inner(e))
            })?;

        debug!("listening on: {}", addr);

        Ok((NoiseListener { listener, keypair: self.keypair, params: self.params }, addr))
    }

    fn dial(self, mut addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let pubkey =
            match addr.pop() {
                Some(Protocol::Curve25519(key)) => key.into_owned(),
                Some(proto) => {
                    addr.append(proto);
                    return Err(TransportError::MultiaddrNotSupported(addr))
                }
                None => return Err(TransportError::MultiaddrNotSupported(addr))
            };

        debug!("dialing {}", addr);

        let dial = self.transport.dial(addr)
            .map_err(|e| match e {
                TransportError::MultiaddrNotSupported(a) => TransportError::MultiaddrNotSupported(a),
                TransportError::Other(e) => TransportError::Other(NoiseError::Inner(e))
            })?;

        debug!("creating session with {}", PublicKey(pubkey).base58_encoded());

        let session = snow::Builder::new(self.params.clone())
            .local_private_key(self.keypair.secret())
            .remote_public_key(&pubkey)
            .build_initiator()
            .map_err(|e| TransportError::Other(NoiseError::Noise(e)))?;

        Ok(NoiseDialFuture(DialState::Init(dial, session)))
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.transport.nat_traversal(server, observed)
    }
}

pub struct NoiseListener<T> {
    listener: T,
    keypair: Arc<Keypair>,
    params: snow::params::NoiseParams
}

impl<T, F> Stream for NoiseListener<T>
where
    T: Stream<Item = (F, Multiaddr)>,
    F: Future,
    F::Item: AsyncRead + AsyncWrite
{
    type Item = (NoiseListenFuture<F>, Multiaddr);
    type Error= NoiseError<T::Error>;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let Some((future, addr)) = try_ready!(self.listener.poll().map_err(NoiseError::Inner)) {
            trace!("incoming stream: creating new session");
            let session = snow::Builder::new(self.params.clone())
                .local_private_key(self.keypair.secret())
                .build_responder()?;
            Ok(Async::Ready(Some((NoiseListenFuture(ListenState::Init(future, session)), addr))))
        } else {
            Ok(Async::Ready(None))
        }
    }
}

pub struct NoiseListenFuture<T: Future>(ListenState<T>);

enum ListenState<T: Future> {
    Init(T, snow::Session),
    RecvHandshake(NoiseOutput<T::Item>),
    SendHandshake(NoiseOutput<T::Item>),
    Flush(NoiseOutput<T::Item>),
    Done
}

impl<T> Future for NoiseListenFuture<T>
where
    T: Future,
    T::Item: AsyncRead + AsyncWrite
{
    type Item = (PeerId, NoiseOutput<T::Item>);
    type Error = NoiseError<T::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.0, ListenState::Done) {
                ListenState::Init(mut future, session) => {
                    if let Async::Ready(io) = future.poll().map_err(NoiseError::Inner)? {
                        let output = NoiseOutput::new(io, session);
                        self.0 = ListenState::RecvHandshake(output)
                    } else {
                        self.0 = ListenState::Init(future, session);
                        return Ok(Async::NotReady)
                    }
                }
                ListenState::RecvHandshake(mut io) => {
                    // <- e, es, s, ss
                    if io.poll_read(&mut []).map_err(NoiseError::Io)?.is_ready() {
                        self.0 = ListenState::SendHandshake(io)
                    } else {
                        self.0 = ListenState::RecvHandshake(io);
                        return Ok(Async::NotReady)
                    }
                }
                ListenState::SendHandshake(mut io) => {
                    // -> e, ee, se
                    if io.poll_write(&[]).map_err(NoiseError::Io)?.is_ready() {
                        self.0 = ListenState::Flush(io)
                    } else {
                        self.0 = ListenState::SendHandshake(io);
                        return Ok(Async::NotReady)
                    }
                }
                ListenState::Flush(mut io) => {
                    if io.poll_flush().map_err(NoiseError::Io)?.is_ready() {
                        let s = io.session.into_transport_mode()?;
                        let m = s.get_remote_static()
                            .ok_or(NoiseError::InvalidKey)
                            .and_then(to_array)?;
                        let io = NoiseOutput { session: s, .. io };
                        self.0 = ListenState::Done;
                        return Ok(Async::Ready((PeerId::encode(&m), io)))
                    } else {
                        self.0 = ListenState::Flush(io);
                        return Ok(Async::NotReady)
                    }
                }
                ListenState::Done => panic!("NoiseListenFuture::poll called after completion")
            }
        }
    }
}

pub struct NoiseDialFuture<T: Future>(DialState<T>);

enum DialState<T: Future> {
    Init(T, snow::Session),
    SendHandshake(NoiseOutput<T::Item>),
    Flush(NoiseOutput<T::Item>),
    RecvHandshake(NoiseOutput<T::Item>),
    Done
}

impl<T> Future for NoiseDialFuture<T>
where
    T: Future,
    T::Item: AsyncRead + AsyncWrite
{
    type Item = (PeerId, NoiseOutput<T::Item>);
    type Error = NoiseError<T::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.0, DialState::Done) {
                DialState::Init(mut future, session) => {
                    if let Async::Ready(io) = future.poll().map_err(NoiseError::Inner)? {
                        let output = NoiseOutput::new(io, session);
                        self.0 = DialState::SendHandshake(output)
                    } else {
                        self.0 = DialState::Init(future, session);
                        return Ok(Async::NotReady)
                    }
                }
                DialState::SendHandshake(mut io) => {
                    // -> e, es, s, ss
                    if io.poll_write(&[]).map_err(NoiseError::Io)?.is_ready() {
                        self.0 = DialState::Flush(io)
                    } else {
                        self.0 = DialState::SendHandshake(io);
                        return Ok(Async::NotReady)
                    }
                }
                DialState::Flush(mut io) => {
                    if io.poll_flush().map_err(NoiseError::Io)?.is_ready() {
                        self.0 = DialState::RecvHandshake(io)
                    } else {
                        self.0 = DialState::Flush(io);
                        return Ok(Async::NotReady)
                    }
                }
                DialState::RecvHandshake(mut io) => {
                    // <- e, ee, se
                    if io.poll_read(&mut []).map_err(NoiseError::Io)?.is_ready() {
                        let s = io.session.into_transport_mode()?;
                        let m = s.get_remote_static()
                            .ok_or(NoiseError::InvalidKey)
                            .and_then(to_array)?;
                        let io = NoiseOutput { session: s, .. io };
                        self.0 = DialState::Done;
                        return Ok(Async::Ready((PeerId::encode(&m), io)))
                    } else {
                        self.0 = DialState::RecvHandshake(io);
                        return Ok(Async::NotReady)
                    }
                }
                DialState::Done => panic!("NoiseDialFuture::poll called after completion")
            }
        }
    }
}

fn to_array<E>(bytes: &[u8]) -> Result<[u8; 32], NoiseError<E>> {
    if bytes.len() != 32 {
        return Err(NoiseError::InvalidKey)
    }
    let mut m = [0; 32];
    m.copy_from_slice(bytes);
    Ok(m)
}

