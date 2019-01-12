// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! Provides the `BrahmsRequest` upgrade that sends a request on the network and waits for a
//! potential response, and `BrahmsListen` upgrade that accepts a request from the remote.

use crate::codec::{Codec, RawMessage};
use crate::pow::Pow;
use futures::{prelude::*, try_ready};
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_core::{Multiaddr, PeerId};
use std::{error, io, iter};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};

/// Request that can be sent to a peer.
#[derive(Debug, Clone)]
pub struct BrahmsPushRequest {
    /// Id of the local peer.
    pub local_peer_id: PeerId,
    /// Id of the peer we're going to send this message to. The message is only valid for this
    /// specific peer.
    pub remote_peer_id: PeerId,
    /// Addresses we're listening on.
    pub addresses: Vec<Multiaddr>,
    /// Difficulty of the proof of work.
    pub pow_difficulty: u8,
}

impl UpgradeInfo for BrahmsPushRequest {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/paritytech/brahms/0.1.0")
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for BrahmsPushRequest
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = ();
    type Error = io::Error;
    type Future = BrahmsPushRequestFlush<TSocket>;

    #[inline]
    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        let addrs = self
            .addresses
            .into_iter()
            .map(Multiaddr::into_bytes)
            .collect();
        let pow = Pow::generate(&self.local_peer_id, &self.remote_peer_id, self.pow_difficulty).unwrap();   // TODO:
        let message = RawMessage::Push(addrs, pow.nonce());
        // TODO: what if lots of addrs? https://github.com/libp2p/rust-libp2p/issues/760
        BrahmsPushRequestFlush {
            inner: Framed::new(socket, Codec::default()),
            message: Some(message),
        }
    }
}

/// Future that sends a push request to the remote.
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct BrahmsPushRequestFlush<TSocket> {
    /// The stream to the remote.
    inner: Framed<TSocket, Codec>,
    /// The message to send back to the remote.
    message: Option<RawMessage>,
}

impl<TSocket> Future for BrahmsPushRequestFlush<TSocket>
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(message) = self.message.take() {
            match self.inner.start_send(message)? {
                AsyncSink::Ready => (),
                AsyncSink::NotReady(message) => {
                    self.message = Some(message);
                    return Ok(Async::NotReady);
                }
            }
        }

        try_ready!(self.inner.poll_complete());
        Ok(Async::Ready(()))
    }
}

/// Request that can be sent to a peer.
#[derive(Debug, Clone)]
pub struct BrahmsPullRequestRequest {}

impl UpgradeInfo for BrahmsPullRequestRequest {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/paritytech/brahms/0.1.0")
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for BrahmsPullRequestRequest
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = Vec<(PeerId, Vec<Multiaddr>)>;
    type Error = Box<error::Error + Send + Sync>;
    type Future = BrahmsPullRequestRequestFlush<TSocket>;

    #[inline]
    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        BrahmsPullRequestRequestFlush {
            inner: Framed::new(socket, Codec::default()),
            message: Some(RawMessage::PullRequest),
            flushed: false,
        }
    }
}

/// Future that sends a pull request request to the remote, and waits for an answer.
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct BrahmsPullRequestRequestFlush<TSocket> {
    /// The stream to the remote.
    inner: Framed<TSocket, Codec>,
    /// The message to send back to the remote.
    message: Option<RawMessage>,
    /// If true, then we successfully flushed after sending.
    flushed: bool,
}

impl<TSocket> Future for BrahmsPullRequestRequestFlush<TSocket>
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Item = Vec<(PeerId, Vec<Multiaddr>)>;
    type Error = Box<error::Error + Send + Sync>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(message) = self.message.take() {
            match self.inner.start_send(message)? {
                AsyncSink::Ready => (),
                AsyncSink::NotReady(message) => {
                    self.message = Some(message);
                    return Ok(Async::NotReady);
                }
            }
        }

        if !self.flushed {
            try_ready!(self.inner.poll_complete());
            self.flushed = true;
        }

        match try_ready!(self.inner.poll()) {
            Some(RawMessage::PullResponse(response)) => {
                let mut out = Vec::new();
                for (peer_id, addrs) in response {
                    let peer_id = if let Ok(id) = PeerId::from_bytes(peer_id) {
                        id
                    } else {
                        return Err("Invalid peer ID in pull response".to_string().into());
                    };

                    let mut peer = Vec::new();
                    for addr in addrs {
                        if let Ok(a) = Multiaddr::from_bytes(addr) {
                            peer.push(a);
                        } else {
                            return Err("Invalid multiaddr in pull response".to_string().into());
                        }
                    }
                    out.push((peer_id, peer));
                }
                Ok(Async::Ready(out))
            }
            Some(RawMessage::Push(_, _)) | Some(RawMessage::PullRequest) | None => {
                Err("Invalid remote request".to_string().into())
            }
        }
    }
}

/// Upgrade that listens for a request from the remote, and allows answering it.
#[derive(Debug, Clone)]
pub struct BrahmsListen {
    /// Id of the local peer. Messages not received by this id can be invalid.
    pub local_peer_id: PeerId,
    /// Id of the peer that sends us messages. Messages not sent by this id can be invalid.
    pub remote_peer_id: PeerId,
    /// Required difficulty of the proof of work.
    pub pow_difficulty: u8,
}

impl UpgradeInfo for BrahmsListen {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/paritytech/brahms/0.1.0")
    }
}

impl<TSocket> InboundUpgrade<TSocket> for BrahmsListen
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Output = BrahmsListenOut<TSocket>;
    type Error = Box<error::Error + Send + Sync>;
    type Future = BrahmsListenFuture<TSocket>;

    #[inline]
    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        BrahmsListenFuture {
            inner: Some(Framed::new(socket, Codec::default())),
            local_peer_id: self.local_peer_id,
            remote_peer_id: self.remote_peer_id,
            pow_difficulty: self.pow_difficulty,
        }
    }
}

/// Future that listens for a query from the remote.
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct BrahmsListenFuture<TSocket> {
    /// The stream to the remote.
    inner: Option<Framed<TSocket, Codec>>,
    /// Id of the local peer. The message is only valid for this specific peer.
    local_peer_id: PeerId,
    /// Id of the peer we're going to send this message to. The message is only valid for this
    /// specific peer.
    remote_peer_id: PeerId,
    /// Required difficulty of the proof of work.
    pow_difficulty: u8,
}

impl<TSocket> Future for BrahmsListenFuture<TSocket>
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Item = BrahmsListenOut<TSocket>;
    type Error = Box<error::Error + Send + Sync>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match try_ready!(self
            .inner
            .as_mut()
            .expect("Future is already finished")
            .poll())
        {
            Some(RawMessage::Push(addrs, nonce)) => {
                if Pow::verify(&self.local_peer_id, &self.remote_peer_id, nonce, self.pow_difficulty).is_err() {
                    return Err(io::Error::new(io::ErrorKind::Other, "invalid PoW").into());
                }
                let mut addrs_parsed = Vec::with_capacity(addrs.len());
                for addr in addrs {
                    addrs_parsed.push(Multiaddr::from_bytes(addr)?);
                }
                Ok(Async::Ready(BrahmsListenOut::Push(addrs_parsed)))
            }
            Some(RawMessage::PullRequest) => Ok(Async::Ready(BrahmsListenOut::PullRequest(
                BrahmsListenPullRequest {
                    inner: self.inner.take().expect("Future is already finished"),
                },
            ))),
            Some(RawMessage::PullResponse(_)) | None => {
                Err("Invalid remote request".to_string().into())
            }
        }
    }
}

/// Request received from a remote.
#[derive(Debug)]
pub enum BrahmsListenOut<TSocket> {
    /// The sender pushes itself to us. Contains the addresses it's listening on.
    Push(Vec<Multiaddr>),

    /// Sender requests us to send back our view of the network.
    PullRequest(BrahmsListenPullRequest<TSocket>),
}

/// Sender requests us to send back our view of the network.
#[derive(Debug)]
pub struct BrahmsListenPullRequest<TSocket> {
    inner: Framed<TSocket, Codec>,
}

impl<TSocket> BrahmsListenPullRequest<TSocket> {
    /// Respond to the request.
    pub fn respond(
        self,
        view: impl IntoIterator<Item = (PeerId, impl IntoIterator<Item = Multiaddr>)>,
    ) -> BrahmsListenPullRequestFlush<TSocket> {
        let view = view
            .into_iter()
            .map(|(peer_id, addrs)| {
                (
                    peer_id.into_bytes(),
                    addrs.into_iter().map(|addr| addr.into_bytes()).collect(),
                )
            })
            .collect();

        BrahmsListenPullRequestFlush {
            inner: self.inner,
            message: Some(RawMessage::PullResponse(view)),
        }
    }
}

/// Future that answers a pull request from the remote.
#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct BrahmsListenPullRequestFlush<TSocket> {
    /// The stream to the remote.
    inner: Framed<TSocket, Codec>,
    /// The message to send back to the remote.
    message: Option<RawMessage>,
}

impl<TSocket> Future for BrahmsListenPullRequestFlush<TSocket>
where
    TSocket: AsyncRead + AsyncWrite,
{
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if let Some(message) = self.message.take() {
            match self.inner.start_send(message)? {
                AsyncSink::Ready => (),
                AsyncSink::NotReady(message) => {
                    self.message = Some(message);
                    return Ok(Async::NotReady);
                }
            }
        }

        try_ready!(self.inner.poll_complete());
        Ok(Async::Ready(()))
    }
}
