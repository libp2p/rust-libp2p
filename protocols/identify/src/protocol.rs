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

use bytes::{Bytes, BytesMut};
use futures::{future, Future, Sink, Stream};
use libp2p_core::{ConnectionUpgrade, Endpoint, Multiaddr, PublicKey};
use protobuf::Message as ProtobufMessage;
use protobuf::parse_from_bytes as protobuf_parse_from_bytes;
use protobuf::RepeatedField;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use structs_proto;
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec;

/// Configuration for an upgrade to the identity protocol.
#[derive(Debug, Clone)]
pub struct IdentifyProtocolConfig;

/// Output of the connection upgrade.
pub enum IdentifyOutput<T> {
    /// We obtained information from the remote. Happens when we are the dialer.
    RemoteInfo {
        /// Information about the remote.
        info: IdentifyInfo,
        /// Address the remote sees for us.
        observed_addr: Multiaddr,
    },

    /// We opened a connection to the remote and need to send it information. Happens when we are
    /// the listener.
    Sender {
        /// Object used to send identify info to the client.
        sender: IdentifySender<T>,
    },
}

/// Object used to send back information to the client.
pub struct IdentifySender<T> {
    inner: Framed<T, codec::UviBytes<Vec<u8>>>,
}

impl<'a, T> IdentifySender<T>
where
    T: AsyncWrite + Send + 'a,
{
    /// Sends back information to the client. Returns a future that is signalled whenever the
    /// info have been sent.
    pub fn send(
        self,
        info: IdentifyInfo,
        observed_addr: &Multiaddr,
    ) -> Box<Future<Item = (), Error = IoError> + Send + 'a> {
        debug!("Sending identify info to client");
        trace!("Sending: {:?}", info);

        let listen_addrs = info.listen_addrs
            .into_iter()
            .map(|addr| addr.into_bytes())
            .collect();

        let mut message = structs_proto::Identify::new();
        message.set_agentVersion(info.agent_version);
        message.set_protocolVersion(info.protocol_version);
        message.set_publicKey(info.public_key.into_protobuf_encoding());
        message.set_listenAddrs(listen_addrs);
        message.set_observedAddr(observed_addr.to_bytes());
        message.set_protocols(RepeatedField::from_vec(info.protocols));

        let bytes = message
            .write_to_bytes()
            .expect("writing protobuf failed ; should never happen");

        let future = self.inner.send(bytes).map(|_| ());
        Box::new(future) as Box<_>
    }
}

/// Information sent from the listener to the dialer.
#[derive(Debug, Clone)]
pub struct IdentifyInfo {
    /// Public key of the node.
    pub public_key: PublicKey,
    /// Version of the "global" protocol, eg. `ipfs/1.0.0` or `polkadot/1.0.0`.
    pub protocol_version: String,
    /// Name and version of the client. Can be thought as similar to the `User-Agent` header
    /// of HTTP.
    pub agent_version: String,
    /// Addresses that the node is listening on.
    pub listen_addrs: Vec<Multiaddr>,
    /// Protocols supported by the node, eg. `/ipfs/ping/1.0.0`.
    pub protocols: Vec<String>,
}

impl<C> ConnectionUpgrade<C> for IdentifyProtocolConfig
where
    C: AsyncRead + AsyncWrite + Send + 'static,
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();
    type Output = IdentifyOutput<C>;
    type Future = Box<Future<Item = Self::Output, Error = IoError> + Send>;

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/ipfs/id/1.0.0"), ()))
    }

    fn upgrade(self, socket: C, _: (), ty: Endpoint) -> Self::Future {
        trace!("Upgrading connection as {:?}", ty);

        let socket = Framed::new(socket, codec::UviBytes::default());

        match ty {
            Endpoint::Dialer => {
                let future = socket
                    .into_future()
                    .map(|(msg, _)| msg)
                    .map_err(|(err, _)| err)
                    .and_then(|msg| {
                        debug!("Received identify message");

                        if let Some(msg) = msg {
                            let (info, observed_addr) = match parse_proto_msg(msg) {
                                Ok(v) => v,
                                Err(err) => {
                                    debug!("Failed to parse protobuf message ; error = {:?}", err);
                                    return Err(err.into());
                                }
                            };

                            trace!("Remote observes us as {:?}", observed_addr);
                            trace!("Information received: {:?}", info);

                            Ok(IdentifyOutput::RemoteInfo {
                                info,
                                observed_addr: observed_addr.clone(),
                            })
                        } else {
                            debug!("Identify protocol stream closed before receiving info");
                            Err(IoErrorKind::InvalidData.into())
                        }
                    });

                Box::new(future) as Box<_>
            }

            Endpoint::Listener => {
                let sender = IdentifySender { inner: socket };
                let future = future::ok(IdentifyOutput::Sender { sender });
                Box::new(future) as Box<_>
            }
        }
    }
}

// Turns a protobuf message into an `IdentifyInfo` and an observed address. If something bad
// happens, turn it into an `IoError`.
fn parse_proto_msg(msg: BytesMut) -> Result<(IdentifyInfo, Multiaddr), IoError> {
    match protobuf_parse_from_bytes::<structs_proto::Identify>(&msg) {
        Ok(mut msg) => {
            // Turn a `Vec<u8>` into a `Multiaddr`. If something bad happens, turn it into
            // an `IoError`.
            fn bytes_to_multiaddr(bytes: Vec<u8>) -> Result<Multiaddr, IoError> {
                Multiaddr::from_bytes(bytes)
                    .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))
            }

            let listen_addrs = {
                let mut addrs = Vec::new();
                for addr in msg.take_listenAddrs().into_iter() {
                    addrs.push(bytes_to_multiaddr(addr)?);
                }
                addrs
            };

            let observed_addr = bytes_to_multiaddr(msg.take_observedAddr())?;
            let info = IdentifyInfo {
                public_key: PublicKey::from_protobuf_encoding(msg.get_publicKey())?,
                protocol_version: msg.take_protocolVersion(),
                agent_version: msg.take_agentVersion(),
                listen_addrs: listen_addrs,
                protocols: msg.take_protocols().into_vec(),
            };

            Ok((info, observed_addr))
        }

        Err(err) => Err(IoError::new(IoErrorKind::InvalidData, err)),
    }
}

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp_transport;
    extern crate tokio_current_thread;

    use self::libp2p_tcp_transport::TcpConfig;
    use futures::{Future, Stream};
    use libp2p_core::{PublicKey, Transport};
    use std::sync::mpsc;
    use std::thread;
    use {IdentifyInfo, IdentifyOutput, IdentifyProtocolConfig};

    #[test]
    fn correct_transfer() {
        // We open a server and a client, send info from the server to the client, and check that
        // they were successfully received.

        let (tx, rx) = mpsc::channel();

        let bg_thread = thread::spawn(move || {
            let transport = TcpConfig::new().with_upgrade(IdentifyProtocolConfig);

            let (listener, addr) = transport
                .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                .unwrap();
            tx.send(addr).unwrap();

            let future = listener
                .into_future()
                .map_err(|(err, _)| err)
                .and_then(|(client, _)| client.unwrap().0)
                .and_then(|identify| match identify {
                    IdentifyOutput::Sender { sender, .. } => sender.send(
                        IdentifyInfo {
                            public_key: PublicKey::Ed25519(vec![1, 2, 3, 4, 5, 7]),
                            protocol_version: "proto_version".to_owned(),
                            agent_version: "agent_version".to_owned(),
                            listen_addrs: vec![
                                "/ip4/80.81.82.83/tcp/500".parse().unwrap(),
                                "/ip6/::1/udp/1000".parse().unwrap(),
                            ],
                            protocols: vec!["proto1".to_string(), "proto2".to_string()],
                        },
                        &"/ip4/100.101.102.103/tcp/5000".parse().unwrap(),
                    ),
                    _ => panic!(),
                });

            let _ = tokio_current_thread::block_on_all(future).unwrap();
        });

        let transport = TcpConfig::new().with_upgrade(IdentifyProtocolConfig);

        let future = transport
            .dial(rx.recv().unwrap())
            .unwrap_or_else(|_| panic!())
            .and_then(|identify| match identify {
                IdentifyOutput::RemoteInfo {
                    info,
                    observed_addr,
                } => {
                    assert_eq!(
                        observed_addr,
                        "/ip4/100.101.102.103/tcp/5000".parse().unwrap()
                    );
                    assert_eq!(info.public_key, PublicKey::Ed25519(vec![1, 2, 3, 4, 5, 7]));
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
                    Ok(())
                }
                _ => panic!(),
            });

        let _ = tokio_current_thread::block_on_all(future).unwrap();
        bg_thread.join().unwrap();
    }
}
