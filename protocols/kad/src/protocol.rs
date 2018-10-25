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

//! Provides the `KadMsg` enum of all the possible messages transmitted with the Kademlia protocol,
//! and the `KademliaProtocolConfig` connection upgrade whose output is a
//! `Stream<Item = KadMsg> + Sink<SinkItem = KadMsg>`.
//!
//! The `Stream` component is used to poll the underlying transport, and the `Sink` component is
//! used to send messages.

use bytes::{Bytes, BytesMut};
use futures::{future, sink, Sink, stream, Stream};
use libp2p_core::{ConnectionUpgrade, Endpoint, Multiaddr, PeerId};
use multihash::Multihash;
use protobuf::{self, Message};
use protobuf_structs;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec;

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum KadConnectionType {
    /// Sender hasn't tried to connect to peer.
    NotConnected = 0,
    /// Sender is currently connected to peer.
    Connected = 1,
    /// Sender was recently connected to peer.
    CanConnect = 2,
    /// Sender tried to connect to peer but failed.
    CannotConnect = 3,
}

impl From<protobuf_structs::dht::Message_ConnectionType> for KadConnectionType {
    #[inline]
    fn from(raw: protobuf_structs::dht::Message_ConnectionType) -> KadConnectionType {
        use protobuf_structs::dht::Message_ConnectionType::*;
        match raw {
            NOT_CONNECTED => KadConnectionType::NotConnected,
            CONNECTED => KadConnectionType::Connected,
            CAN_CONNECT => KadConnectionType::CanConnect,
            CANNOT_CONNECT => KadConnectionType::CannotConnect,
        }
    }
}

impl Into<protobuf_structs::dht::Message_ConnectionType> for KadConnectionType {
    #[inline]
    fn into(self) -> protobuf_structs::dht::Message_ConnectionType {
        use protobuf_structs::dht::Message_ConnectionType::*;
        match self {
            KadConnectionType::NotConnected => NOT_CONNECTED,
            KadConnectionType::Connected => CONNECTED,
            KadConnectionType::CanConnect => CAN_CONNECT,
            KadConnectionType::CannotConnect => CANNOT_CONNECT,
        }
    }
}

/// Information about a peer, as known by the sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KadPeer {
    pub node_id: PeerId,
    /// The multiaddresses that are known for that peer.
    pub multiaddrs: Vec<Multiaddr>,
    pub connection_ty: KadConnectionType,
}

impl KadPeer {
    // Builds a `KadPeer` from its raw protobuf equivalent.
    // TODO: use TryFrom once stable
    fn from_peer(peer: &mut protobuf_structs::dht::Message_Peer) -> Result<KadPeer, IoError> {
        // TODO: this is in fact a CID ; not sure if this should be handled in `from_bytes` or
        //       as a special case here
        let node_id = PeerId::from_bytes(peer.get_id().to_vec())
            .map_err(|_| IoError::new(IoErrorKind::InvalidData, "invalid peer id"))?;

        let mut addrs = Vec::with_capacity(peer.get_addrs().len());
        for addr in peer.take_addrs().into_iter() {
            let as_ma = Multiaddr::from_bytes(addr)
                .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))?;
            addrs.push(as_ma);
        }
        debug_assert_eq!(addrs.len(), addrs.capacity());

        let connection_ty = peer.get_connection().into();

        Ok(KadPeer {
            node_id: node_id,
            multiaddrs: addrs,
            connection_ty: connection_ty,
        })
    }
}

impl Into<protobuf_structs::dht::Message_Peer> for KadPeer {
    fn into(self) -> protobuf_structs::dht::Message_Peer {
        let mut out = protobuf_structs::dht::Message_Peer::new();
        out.set_id(self.node_id.into_bytes());
        for addr in self.multiaddrs {
            out.mut_addrs().push(addr.into_bytes());
        }
        out.set_connection(self.connection_ty.into());
        out
    }
}

/// Configuration for a Kademlia connection upgrade. When applied to a connection, turns this
/// connection into a `Stream + Sink` whose items are of type `KadMsg`.
#[derive(Debug, Default, Copy, Clone)]
pub struct KademliaProtocolConfig;

impl<C> ConnectionUpgrade<C> for KademliaProtocolConfig
where
    C: AsyncRead + AsyncWrite + 'static, // TODO: 'static :-/
{
    type Output = KadStreamSink<C>;
    type Future = future::FutureResult<Self::Output, IoError>;
    type NamesIter = iter::Once<(Bytes, ())>;
    type UpgradeIdentifier = ();

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once(("/ipfs/kad/1.0.0".into(), ()))
    }

    #[inline]
    fn upgrade(self, incoming: C, _: (), _: Endpoint) -> Self::Future {
        future::ok(kademlia_protocol(incoming))
    }
}

type KadStreamSink<S> = stream::AndThen<sink::With<stream::FromErr<Framed<S, codec::UviBytes<Vec<u8>>>, IoError>, KadMsg, fn(KadMsg) -> Result<Vec<u8>, IoError>, Result<Vec<u8>, IoError>>, fn(BytesMut) -> Result<KadMsg, IoError>, Result<KadMsg, IoError>>;

// Upgrades a socket to use the Kademlia protocol.
fn kademlia_protocol<S>(
    socket: S,
) -> KadStreamSink<S>
where
    S: AsyncRead + AsyncWrite,
{
    Framed::new(socket, codec::UviBytes::default())
        .from_err::<IoError>()
        .with::<_, fn(_) -> _, _>(|request| -> Result<_, IoError> {
            let proto_struct = msg_to_proto(request);
            Ok(proto_struct.write_to_bytes().unwrap()) // TODO: error?
        })
        .and_then::<fn(_) -> _, _>(|bytes| {
            let response = protobuf::parse_from_bytes(&bytes)?;
            proto_to_msg(response)
        })
}

/// Message that we can send to a peer or received from a peer.
// TODO: document the rest
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KadMsg {
    /// Ping request or response.
    Ping,

    /// Target must save the given record, can be queried later with `GetValueReq`.
    PutValue {
        /// Identifier of the record.
        key: Multihash,
        /// The record itself.
        record: (), //record: protobuf_structs::record::Record, // TODO: no
    },

    GetValueReq {
        /// Identifier of the record.
        key: Multihash,
    },

    GetValueRes {
        /// Identifier of the returned record.
        key: Multihash,
        record: (), //record: Option<protobuf_structs::record::Record>, // TODO: no
        closer_peers: Vec<KadPeer>,
    },

    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNodeReq {
        /// Identifier of the node.
        key: PeerId,
    },

    /// Response to a `FindNodeReq`.
    FindNodeRes {
        /// Results of the request.
        closer_peers: Vec<KadPeer>,
    },

    /// Same as `FindNodeReq`, but should also return the entries of the local providers list for
    /// this key.
    GetProvidersReq {
        /// Identifier being searched.
        key: Multihash,
    },

    /// Response to a `FindNodeReq`.
    GetProvidersRes {
        /// Nodes closest to the key.
        closer_peers: Vec<KadPeer>,
        /// Known providers for this key.
        provider_peers: Vec<KadPeer>,
    },

    /// Indicates that this list of providers is known for this key.
    AddProvider {
        /// Key for which we should add providers.
        key: Multihash,
        /// Known provider for this key.
        provider_peer: KadPeer,
    },
}

// Turns a type-safe kadmelia message into the corresponding row protobuf message.
fn msg_to_proto(kad_msg: KadMsg) -> protobuf_structs::dht::Message {
    match kad_msg {
        KadMsg::Ping => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::PING);
            msg
        }
        KadMsg::PutValue { key, .. } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::PUT_VALUE);
            msg.set_key(key.into_bytes());
            //msg.set_record(record);		// TODO:
            msg
        }
        KadMsg::GetValueReq { key } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::GET_VALUE);
            msg.set_key(key.into_bytes());
            msg.set_clusterLevelRaw(10);
            msg
        }
        KadMsg::GetValueRes { .. } => unimplemented!(),     // TODO:
        KadMsg::FindNodeReq { key } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::FIND_NODE);
            msg.set_key(key.into_bytes());
            msg.set_clusterLevelRaw(10);
            msg
        }
        KadMsg::FindNodeRes { closer_peers } => {
            // TODO: if empty, the remote will think it's a request
            // TODO: not good, possibly exposed in the API
            assert!(!closer_peers.is_empty());
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::FIND_NODE);
            msg.set_clusterLevelRaw(9);
            for peer in closer_peers {
                msg.mut_closerPeers().push(peer.into());
            }
            msg
        }
        KadMsg::GetProvidersReq { key } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::GET_PROVIDERS);
            msg.set_key(key.into_bytes());
            msg.set_clusterLevelRaw(10);
            msg
        }
        KadMsg::GetProvidersRes { closer_peers, provider_peers } => {
            // TODO: if empty, the remote will think it's a request
            // TODO: not good, possibly exposed in the API
            assert!(!closer_peers.is_empty());
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::GET_PROVIDERS);
            msg.set_clusterLevelRaw(9);
            for peer in closer_peers {
                msg.mut_closerPeers().push(peer.into());
            }
            for peer in provider_peers {
                msg.mut_providerPeers().push(peer.into());
            }
            msg
        }
        KadMsg::AddProvider { key, provider_peer } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::ADD_PROVIDER);
            msg.set_clusterLevelRaw(10);
            msg.set_key(key.into_bytes());
            msg.mut_providerPeers().push(provider_peer.into());
            msg
        }
    }
}

/// Turns a raw Kademlia message into a type-safe message.
fn proto_to_msg(mut message: protobuf_structs::dht::Message) -> Result<KadMsg, IoError> {
    match message.get_field_type() {
        protobuf_structs::dht::Message_MessageType::PING => Ok(KadMsg::Ping),

        protobuf_structs::dht::Message_MessageType::PUT_VALUE => {
            let key = Multihash::from_bytes(message.take_key())
                .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))?;
            let _record = message.take_record();
            Ok(KadMsg::PutValue {
                key: key,
                record: (),
            })
        }

        protobuf_structs::dht::Message_MessageType::GET_VALUE => {
            let key = Multihash::from_bytes(message.take_key())
                .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))?;
            Ok(KadMsg::GetValueReq { key: key })
        }

        protobuf_structs::dht::Message_MessageType::FIND_NODE => {
            if message.get_closerPeers().is_empty() {
                let key = PeerId::from_bytes(message.take_key())
                    .map_err(|_| IoError::new(IoErrorKind::InvalidData, "invalid peer id in FIND_NODE"))?;
                Ok(KadMsg::FindNodeReq {
                    key,
                })

            } else {
                // TODO: for now we don't parse the peer properly, so it is possible that we get
                //       parsing errors for peers even when they are valid ; we ignore these
                //       errors for now, but ultimately we should just error altogether
                let closer_peers = message.mut_closerPeers()
                    .iter_mut()
                    .filter_map(|peer| KadPeer::from_peer(peer).ok())
                    .collect::<Vec<_>>();

                Ok(KadMsg::FindNodeRes {
                    closer_peers,
                })
            }
        }

        protobuf_structs::dht::Message_MessageType::GET_PROVIDERS => {
            if message.get_closerPeers().is_empty() {
                let key = Multihash::from_bytes(message.take_key())
                    .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))?;
                Ok(KadMsg::GetProvidersReq {
                    key,
                })

            } else {
                // TODO: for now we don't parse the peer properly, so it is possible that we get
                //       parsing errors for peers even when they are valid ; we ignore these
                //       errors for now, but ultimately we should just error altogether
                let closer_peers = message.mut_closerPeers()
                    .iter_mut()
                    .filter_map(|peer| KadPeer::from_peer(peer).ok())
                    .collect::<Vec<_>>();
                let provider_peers = message.mut_providerPeers()
                    .iter_mut()
                    .filter_map(|peer| KadPeer::from_peer(peer).ok())
                    .collect::<Vec<_>>();

                Ok(KadMsg::GetProvidersRes {
                    closer_peers,
                    provider_peers,
                })
            }
        }

        protobuf_structs::dht::Message_MessageType::ADD_PROVIDER => {
            // TODO: for now we don't parse the peer properly, so it is possible that we get
            //       parsing errors for peers even when they are valid ; we ignore these
            //       errors for now, but ultimately we should just error altogether
            let provider_peer = message.mut_providerPeers()
                .iter_mut()
                .filter_map(|peer| KadPeer::from_peer(peer).ok())
                .next();

            if let Some(provider_peer) = provider_peer {
                let key = Multihash::from_bytes(message.take_key())
                    .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))?;
                Ok(KadMsg::AddProvider {
                    key,
                    provider_peer,
                })
            } else {
                Err(IoError::new(
                    IoErrorKind::InvalidData,
                    "received an ADD_PROVIDER message with no valid peer",
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp_transport;
    extern crate tokio;

    use self::libp2p_tcp_transport::TcpConfig;
    use futures::{Future, Sink, Stream};
    use libp2p_core::{Transport, PeerId, PublicKey};
    use multihash::{encode, Hash};
    use protocol::{KadConnectionType, KadMsg, KademliaProtocolConfig, KadPeer};
    use std::sync::mpsc;
    use std::thread;
    use self::tokio::runtime::current_thread::Runtime;


    #[test]
    fn correct_transfer() {
        // We open a server and a client, send a message between the two, and check that they were
        // successfully received.

        test_one(KadMsg::Ping);
        test_one(KadMsg::PutValue {
            key: encode(Hash::SHA2256, &[1, 2, 3, 4]).unwrap(),
            record: (),
        });
        test_one(KadMsg::GetValueReq {
            key: encode(Hash::SHA2256, &[10, 11, 12]).unwrap(),
        });
        test_one(KadMsg::FindNodeReq {
            key: PeerId::from_public_key(PublicKey::Rsa(vec![9, 12, 0, 245, 245, 201, 28, 95]))
        });
        test_one(KadMsg::FindNodeRes {
            closer_peers: vec![
                KadPeer {
                    node_id: PeerId::from_public_key(PublicKey::Rsa(vec![93, 80, 12, 250])),
                    multiaddrs: vec!["/ip4/100.101.102.103/tcp/20105".parse().unwrap()],
                    connection_ty: KadConnectionType::Connected,
                },
            ],
        });
        test_one(KadMsg::GetProvidersReq {
            key: encode(Hash::SHA2256, &[9, 12, 0, 245, 245, 201, 28, 95]).unwrap(),
        });
        test_one(KadMsg::GetProvidersRes {
            closer_peers: vec![
                KadPeer {
                    node_id: PeerId::from_public_key(PublicKey::Rsa(vec![93, 80, 12, 250])),
                    multiaddrs: vec!["/ip4/100.101.102.103/tcp/20105".parse().unwrap()],
                    connection_ty: KadConnectionType::Connected,
                },
            ],
            provider_peers: vec![
                KadPeer {
                    node_id: PeerId::from_public_key(PublicKey::Rsa(vec![12, 90, 1, 28])),
                    multiaddrs: vec!["/ip4/200.201.202.203/tcp/1999".parse().unwrap()],
                    connection_ty: KadConnectionType::NotConnected,
                },
            ],
        });
        test_one(KadMsg::AddProvider {
            key: encode(Hash::SHA2256, &[9, 12, 0, 245, 245, 201, 28, 95]).unwrap(),
            provider_peer: KadPeer {
                node_id: PeerId::from_public_key(PublicKey::Rsa(vec![5, 6, 7, 8])),
                multiaddrs: vec!["/ip4/9.1.2.3/udp/23".parse().unwrap()],
                connection_ty: KadConnectionType::Connected,
            },
        });
        // TODO: all messages

        fn test_one(msg_server: KadMsg) {
            let msg_client = msg_server.clone();
            let (tx, rx) = mpsc::channel();

            let bg_thread = thread::spawn(move || {
                let transport = TcpConfig::new().with_upgrade(KademliaProtocolConfig);

                let (listener, addr) = transport
                    .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                    .unwrap();
                tx.send(addr).unwrap();

                let future = listener
                    .into_future()
                    .map_err(|(err, _)| err)
                    .and_then(|(client, _)| client.unwrap().0)
                    .and_then(|proto| proto.into_future().map_err(|(err, _)| err).map(|(v, _)| v))
                    .map(|recv_msg| {
                        assert_eq!(recv_msg.unwrap(), msg_server);
                        ()
                    });
                let mut rt = Runtime::new().unwrap();
                let _ = rt.block_on(future).unwrap();
            });

            let transport = TcpConfig::new().with_upgrade(KademliaProtocolConfig);

            let future = transport
                .dial(rx.recv().unwrap())
                .unwrap_or_else(|_| panic!())
                .and_then(|proto| proto.send(msg_client))
                .map(|_| ());
            let mut rt = Runtime::new().unwrap();
            let _ = rt.block_on(future).unwrap();
            bg_thread.join().unwrap();
        }
    }
}
