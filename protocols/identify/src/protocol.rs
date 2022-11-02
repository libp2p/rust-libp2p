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

use crate::structs_proto;
use asynchronous_codec::{FramedRead, FramedWrite};
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::{
    identity, multiaddr,
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo},
    Multiaddr, PublicKey,
};
use log::trace;
use std::convert::TryFrom;
use std::{fmt, io, iter, pin::Pin};
use thiserror::Error;
use void::Void;

const MAX_MESSAGE_SIZE_BYTES: usize = 4096;

pub const PROTOCOL_NAME: &[u8; 14] = b"/ipfs/id/1.0.0";

pub const PUSH_PROTOCOL_NAME: &[u8; 19] = b"/ipfs/id/push/1.0.0";

/// Substream upgrade protocol for `/ipfs/id/1.0.0`.
#[derive(Debug, Clone)]
pub struct Protocol;

/// Substream upgrade protocol for `/ipfs/id/push/1.0.0`.
#[derive(Debug, Clone)]
pub struct PushProtocol<T>(T);
pub struct InboundPush();
pub struct OutboundPush(Info);

impl PushProtocol<InboundPush> {
    pub fn inbound() -> Self {
        PushProtocol(InboundPush())
    }
}

impl PushProtocol<OutboundPush> {
    pub fn outbound(info: Info) -> Self {
        PushProtocol(OutboundPush(info))
    }
}

/// Information of a peer sent in protocol messages.
#[derive(Debug, Clone)]
pub struct Info {
    /// The public key of the local peer.
    pub public_key: PublicKey,
    /// Application-specific version of the protocol family used by the peer,
    /// e.g. `ipfs/1.0.0` or `polkadot/1.0.0`.
    pub protocol_version: String,
    /// Name and version of the peer, similar to the `User-Agent` header in
    /// the HTTP protocol.
    pub agent_version: String,
    /// The addresses that the peer is listening on.
    pub listen_addrs: Vec<Multiaddr>,
    /// The list of protocols supported by the peer, e.g. `/ipfs/ping/1.0.0`.
    pub protocols: Vec<String>,
    /// Address observed by or for the remote.
    pub observed_addr: Multiaddr,
}

/// The substream on which a reply is expected to be sent.
pub struct ReplySubstream<T> {
    inner: T,
}

impl<T> fmt::Debug for ReplySubstream<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ReplySubstream").finish()
    }
}

impl<T> ReplySubstream<T>
where
    T: AsyncWrite + Unpin,
{
    /// Sends back the requested information on the substream.
    ///
    /// Consumes the substream, returning a future that resolves
    /// when the reply has been sent on the underlying connection.
    pub async fn send(self, info: Info) -> Result<(), UpgradeError> {
        send(self.inner, info).await.map_err(Into::into)
    }
}

impl UpgradeInfo for Protocol {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PROTOCOL_NAME)
    }
}

impl<C> InboundUpgrade<C> for Protocol {
    type Output = ReplySubstream<C>;
    type Error = UpgradeError;
    type Future = future::Ready<Result<Self::Output, UpgradeError>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        future::ok(ReplySubstream { inner: socket })
    }
}

impl<C> OutboundUpgrade<C> for Protocol
where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = Info;
    type Error = UpgradeError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        recv(socket).boxed()
    }
}

impl<T> UpgradeInfo for PushProtocol<T> {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(PUSH_PROTOCOL_NAME)
    }
}

impl<C> InboundUpgrade<C> for PushProtocol<InboundPush>
where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = BoxFuture<'static, Result<Info, UpgradeError>>;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        // Lazily upgrade stream, thus allowing upgrade to happen within identify's handler.
        future::ok(recv(socket).boxed())
    }
}

impl<C> OutboundUpgrade<C> for PushProtocol<OutboundPush>
where
    C: AsyncWrite + Unpin + Send + 'static,
{
    type Output = ();
    type Error = UpgradeError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, socket: C, _: Self::Info) -> Self::Future {
        send(socket, self.0 .0).boxed()
    }
}

async fn send<T>(io: T, info: Info) -> Result<(), UpgradeError>
where
    T: AsyncWrite + Unpin,
{
    trace!("Sending: {:?}", info);

    let listen_addrs = info
        .listen_addrs
        .into_iter()
        .map(|addr| addr.to_vec())
        .collect();

    let pubkey_bytes = info.public_key.to_protobuf_encoding();

    let message = structs_proto::Identify {
        agent_version: Some(info.agent_version),
        protocol_version: Some(info.protocol_version),
        public_key: Some(pubkey_bytes),
        listen_addrs,
        observed_addr: Some(info.observed_addr.to_vec()),
        protocols: info.protocols,
    };

    let mut framed_io = FramedWrite::new(
        io,
        prost_codec::Codec::<structs_proto::Identify>::new(MAX_MESSAGE_SIZE_BYTES),
    );

    framed_io.send(message).await?;
    framed_io.close().await?;

    Ok(())
}

async fn recv<T>(mut socket: T) -> Result<Info, UpgradeError>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    socket.close().await?;

    let info = FramedRead::new(
        socket,
        prost_codec::Codec::<structs_proto::Identify>::new(MAX_MESSAGE_SIZE_BYTES),
    )
    .next()
    .await
    .ok_or(UpgradeError::StreamClosed)??
    .try_into()?;

    trace!("Received: {:?}", info);

    Ok(info)
}

impl TryFrom<structs_proto::Identify> for Info {
    type Error = UpgradeError;

    fn try_from(msg: structs_proto::Identify) -> Result<Self, Self::Error> {
        fn parse_multiaddr(bytes: Vec<u8>) -> Result<Multiaddr, multiaddr::Error> {
            Multiaddr::try_from(bytes)
        }

        let listen_addrs = {
            let mut addrs = Vec::new();
            for addr in msg.listen_addrs.into_iter() {
                addrs.push(parse_multiaddr(addr)?);
            }
            addrs
        };

        let public_key = PublicKey::from_protobuf_encoding(&msg.public_key.unwrap_or_default())?;

        let observed_addr = parse_multiaddr(msg.observed_addr.unwrap_or_default())?;
        let info = Info {
            public_key,
            protocol_version: msg.protocol_version.unwrap_or_default(),
            agent_version: msg.agent_version.unwrap_or_default(),
            listen_addrs,
            protocols: msg.protocols,
            observed_addr,
        };

        Ok(info)
    }
}

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error(transparent)]
    Codec(#[from] prost_codec::Error),
    #[error("I/O interaction failed")]
    Io(#[from] io::Error),
    #[error("Stream closed")]
    StreamClosed,
    #[error("Failed decoding multiaddr")]
    Multiaddr(#[from] multiaddr::Error),
    #[error("Failed decoding public key")]
    PublicKey(#[from] identity::error::DecodingError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::channel::oneshot;
    use libp2p::tcp;
    use libp2p_core::{
        identity,
        upgrade::{self, apply_inbound, apply_outbound},
        Transport,
    };

    #[test]
    fn correct_transfer() {
        // We open a server and a client, send info from the server to the client, and check that
        // they were successfully received.
        let send_pubkey = identity::Keypair::generate_ed25519().public();
        let recv_pubkey = send_pubkey.clone();

        let (tx, rx) = oneshot::channel();

        let bg_task = async_std::task::spawn(async move {
            let mut transport = tcp::async_io::Transport::default().boxed();

            transport
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .unwrap();

            let addr = transport
                .next()
                .await
                .expect("some event")
                .into_new_address()
                .expect("listen address");
            tx.send(addr).unwrap();

            let socket = transport
                .next()
                .await
                .expect("some event")
                .into_incoming()
                .unwrap()
                .0
                .await
                .unwrap();

            let sender = apply_inbound(socket, Protocol).await.unwrap();

            sender
                .send(Info {
                    public_key: send_pubkey,
                    protocol_version: "proto_version".to_owned(),
                    agent_version: "agent_version".to_owned(),
                    listen_addrs: vec![
                        "/ip4/80.81.82.83/tcp/500".parse().unwrap(),
                        "/ip6/::1/udp/1000".parse().unwrap(),
                    ],
                    protocols: vec!["proto1".to_string(), "proto2".to_string()],
                    observed_addr: "/ip4/100.101.102.103/tcp/5000".parse().unwrap(),
                })
                .await
                .unwrap();
        });

        async_std::task::block_on(async move {
            let mut transport = tcp::async_io::Transport::default();

            let socket = transport.dial(rx.await.unwrap()).unwrap().await.unwrap();
            let info = apply_outbound(socket, Protocol, upgrade::Version::V1)
                .await
                .unwrap();
            assert_eq!(
                info.observed_addr,
                "/ip4/100.101.102.103/tcp/5000".parse().unwrap()
            );
            assert_eq!(info.public_key, recv_pubkey);
            assert_eq!(info.protocol_version, "proto_version");
            assert_eq!(info.agent_version, "agent_version");
            assert_eq!(
                info.listen_addrs,
                &[
                    "/ip4/80.81.82.83/tcp/500".parse().unwrap(),
                    "/ip6/::1/udp/1000".parse().unwrap()
                ]
            );
            assert_eq!(
                info.protocols,
                &["proto1".to_string(), "proto2".to_string()]
            );

            bg_task.await;
        });
    }
}
