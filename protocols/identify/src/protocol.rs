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
use futures::prelude::*;
use libp2p_core::{
    Multiaddr,
    PublicKey,
    upgrade::{self, InboundUpgrade, OutboundUpgrade, UpgradeInfo}
};
use log::{debug, trace};
use protobuf::Message as ProtobufMessage;
use protobuf::parse_from_bytes as protobuf_parse_from_bytes;
use protobuf::RepeatedField;
use std::convert::TryFrom;
use std::{fmt, io, iter, pin::Pin};

/// Configuration for an upgrade to the `Identify` protocol.
#[derive(Debug, Clone)]
pub struct IdentifyProtocolConfig;

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    /// Information about the remote.
    pub info: IdentifyInfo,
    /// Address the remote sees for us.
    pub observed_addr: Multiaddr,

    _priv: ()
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
    T: AsyncWrite + Unpin
{
    /// Sends back the requested information on the substream.
    ///
    /// Consumes the substream, returning a `ReplyFuture` that resolves
    /// when the reply has been sent on the underlying connection.
    pub fn send(mut self, info: IdentifyInfo, observed_addr: &Multiaddr)
        -> impl Future<Output = Result<(), io::Error>>
    {
        debug!("Sending identify info to client");
        trace!("Sending: {:?}", info);

        let listen_addrs = info.listen_addrs
            .into_iter()
            .map(|addr| addr.to_vec())
            .collect();

        let pubkey_bytes = info.public_key.into_protobuf_encoding();

        let mut message = structs_proto::Identify::new();
        message.set_agentVersion(info.agent_version);
        message.set_protocolVersion(info.protocol_version);
        message.set_publicKey(pubkey_bytes);
        message.set_listenAddrs(listen_addrs);
        message.set_observedAddr(observed_addr.to_vec());
        message.set_protocols(RepeatedField::from_vec(info.protocols));

        async move {
            let bytes = message
                .write_to_bytes()
                .expect("writing protobuf failed; should never happen");
            upgrade::write_one(&mut self.inner, &bytes).await
        }
    }
}

/// Information of a peer sent in `Identify` protocol responses.
#[derive(Debug, Clone)]
pub struct IdentifyInfo {
    /// The public key underlying the peer's `PeerId`.
    pub public_key: PublicKey,
    /// Version of the protocol family used by the peer, e.g. `ipfs/1.0.0`
    /// or `polkadot/1.0.0`.
    pub protocol_version: String,
    /// Name and version of the peer, similar to the `User-Agent` header in
    /// the HTTP protocol.
    pub agent_version: String,
    /// The addresses that the peer is listening on.
    pub listen_addrs: Vec<Multiaddr>,
    /// The list of protocols supported by the peer, e.g. `/ipfs/ping/1.0.0`.
    pub protocols: Vec<String>,
}

impl UpgradeInfo for IdentifyProtocolConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/ipfs/id/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for IdentifyProtocolConfig
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    type Output = ReplySubstream<C>;
    type Error = io::Error;
    type Future = future::Ready<Result<Self::Output, io::Error>>;

    fn upgrade_inbound(self, socket: C, _: Self::Info) -> Self::Future {
        trace!("Upgrading inbound connection");
        future::ok(ReplySubstream { inner: socket })
    }
}

impl<C> OutboundUpgrade<C> for IdentifyProtocolConfig
where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    type Output = RemoteInfo;
    type Error = upgrade::ReadOneError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, mut socket: C, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            socket.close().await?;
            let msg = upgrade::read_one(&mut socket, 4096).await?;
            let (info, observed_addr) = match parse_proto_msg(msg) {
                Ok(v) => v,
                Err(err) => {
                    debug!("Failed to parse protobuf message; error = {:?}", err);
                    return Err(err.into())
                }
            };

            trace!("Remote observes us as {:?}", observed_addr);
            trace!("Information received: {:?}", info);

            Ok(RemoteInfo {
                info,
                observed_addr: observed_addr.clone(),
                _priv: ()
            })
        })
    }
}

// Turns a protobuf message into an `IdentifyInfo` and an observed address. If something bad
// happens, turn it into an `io::Error`.
fn parse_proto_msg(msg: impl AsRef<[u8]>) -> Result<(IdentifyInfo, Multiaddr), io::Error> {
    match protobuf_parse_from_bytes::<structs_proto::Identify>(msg.as_ref()) {
        Ok(mut msg) => {
            // Turn a `Vec<u8>` into a `Multiaddr`. If something bad happens, turn it into
            // an `io::Error`.
            fn bytes_to_multiaddr(bytes: Vec<u8>) -> Result<Multiaddr, io::Error> {
                Multiaddr::try_from(bytes)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
            }

            let listen_addrs = {
                let mut addrs = Vec::new();
                for addr in msg.take_listenAddrs().into_iter() {
                    addrs.push(bytes_to_multiaddr(addr)?);
                }
                addrs
            };

            let public_key = PublicKey::from_protobuf_encoding(msg.get_publicKey())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

            let observed_addr = bytes_to_multiaddr(msg.take_observedAddr())?;
            let info = IdentifyInfo {
                public_key,
                protocol_version: msg.take_protocolVersion(),
                agent_version: msg.take_agentVersion(),
                listen_addrs,
                protocols: msg.take_protocols().into_vec(),
            };

            Ok((info, observed_addr))
        }

        Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::{IdentifyInfo, RemoteInfo, IdentifyProtocolConfig};
    use libp2p_tcp::TcpConfig;
    use futures::{prelude::*, channel::oneshot};
    use libp2p_core::{
        identity,
        Transport,
        upgrade::{self, apply_outbound, apply_inbound}
    };

    #[test]
    fn correct_transfer() {
        // We open a server and a client, send info from the server to the client, and check that
        // they were successfully received.
        let send_pubkey = identity::Keypair::generate_ed25519().public();
        let recv_pubkey = send_pubkey.clone();

        let (tx, rx) = oneshot::channel();

        let bg_task = async_std::task::spawn(async move {
            let transport = TcpConfig::new();

            let mut listener = transport
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .unwrap();

            let addr = listener.next().await
                .expect("some event")
                .expect("no error")
                .into_new_address()
                .expect("listen address");
            tx.send(addr).unwrap();

            let socket = listener.next().await.unwrap().unwrap().into_upgrade().unwrap().0.await.unwrap();
            let sender = apply_inbound(socket, IdentifyProtocolConfig).await.unwrap();
            sender.send(
                IdentifyInfo {
                    public_key: send_pubkey,
                    protocol_version: "proto_version".to_owned(),
                    agent_version: "agent_version".to_owned(),
                    listen_addrs: vec![
                        "/ip4/80.81.82.83/tcp/500".parse().unwrap(),
                        "/ip6/::1/udp/1000".parse().unwrap(),
                    ],
                    protocols: vec!["proto1".to_string(), "proto2".to_string()],
                },
                &"/ip4/100.101.102.103/tcp/5000".parse().unwrap(),
            ).await.unwrap();
        });

        async_std::task::block_on(async move {
            let transport = TcpConfig::new();

            let socket = transport.dial(rx.await.unwrap()).unwrap().await.unwrap();
            let RemoteInfo { info, observed_addr, .. } =
                apply_outbound(socket, IdentifyProtocolConfig, upgrade::Version::V1).await.unwrap();
            assert_eq!(observed_addr, "/ip4/100.101.102.103/tcp/5000".parse().unwrap());
            assert_eq!(info.public_key, recv_pubkey);
            assert_eq!(info.protocol_version, "proto_version");
            assert_eq!(info.agent_version, "agent_version");
            assert_eq!(info.listen_addrs,
                &["/ip4/80.81.82.83/tcp/500".parse().unwrap(),
                "/ip6/::1/udp/1000".parse().unwrap()]);
            assert_eq!(info.protocols, &["proto1".to_string(), "proto2".to_string()]);

            bg_task.await;
        });
    }
}
