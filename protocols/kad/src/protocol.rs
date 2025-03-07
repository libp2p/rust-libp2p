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

//! The Kademlia connection protocol upgrade and associated message types.
//!
//! The connection protocol upgrade is provided by [`ProtocolConfig`], with the
//! request and response types [`KadRequestMsg`] and [`KadResponseMsg`], respectively.
//! The upgrade's output is a `Sink + Stream` of messages. The `Stream` component is used
//! to poll the underlying transport for incoming messages, and the `Sink` component
//! is used to send messages to remote peers.

use std::{io, marker::PhantomData, time::Duration};

use asynchronous_codec::{Decoder, Encoder, Framed};
use bytes::BytesMut;
use futures::prelude::*;
use libp2p_core::{
    upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo},
    Multiaddr,
};
use libp2p_identity::PeerId;
use libp2p_swarm::StreamProtocol;
use tracing::debug;
use web_time::Instant;

use crate::{
    proto,
    record::{self, Record},
};

/// The protocol name used for negotiating with multistream-select.
pub(crate) const DEFAULT_PROTO_NAME: StreamProtocol = StreamProtocol::new("/ipfs/kad/1.0.0");
/// The default maximum size for a varint length-delimited packet.
pub(crate) const DEFAULT_MAX_PACKET_SIZE: usize = 16 * 1024;
/// Status of our connection to a node reported by the Kademlia protocol.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum ConnectionType {
    /// Sender hasn't tried to connect to peer.
    NotConnected = 0,
    /// Sender is currently connected to peer.
    Connected = 1,
    /// Sender was recently connected to peer.
    CanConnect = 2,
    /// Sender tried to connect to peer but failed.
    CannotConnect = 3,
}

impl From<proto::ConnectionType> for ConnectionType {
    fn from(raw: proto::ConnectionType) -> ConnectionType {
        use proto::ConnectionType::*;
        match raw {
            NOT_CONNECTED => ConnectionType::NotConnected,
            CONNECTED => ConnectionType::Connected,
            CAN_CONNECT => ConnectionType::CanConnect,
            CANNOT_CONNECT => ConnectionType::CannotConnect,
        }
    }
}

impl From<ConnectionType> for proto::ConnectionType {
    fn from(val: ConnectionType) -> Self {
        use proto::ConnectionType::*;
        match val {
            ConnectionType::NotConnected => NOT_CONNECTED,
            ConnectionType::Connected => CONNECTED,
            ConnectionType::CanConnect => CAN_CONNECT,
            ConnectionType::CannotConnect => CANNOT_CONNECT,
        }
    }
}

/// Information about a peer, as known by the sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KadPeer {
    /// Identifier of the peer.
    pub node_id: PeerId,
    /// The multiaddresses that the sender think can be used in order to reach the peer.
    pub multiaddrs: Vec<Multiaddr>,
    /// How the sender is connected to that remote.
    pub connection_ty: ConnectionType,
}

// Builds a `KadPeer` from a corresponding protobuf message.
impl TryFrom<proto::Peer> for KadPeer {
    type Error = io::Error;

    fn try_from(peer: proto::Peer) -> Result<KadPeer, Self::Error> {
        // TODO: this is in fact a CID; not sure if this should be handled in `from_bytes` or
        //       as a special case here
        let node_id = PeerId::from_bytes(&peer.id).map_err(|_| invalid_data("invalid peer id"))?;

        let mut addrs = Vec::with_capacity(peer.addrs.len());
        for addr in peer.addrs.into_iter() {
            match Multiaddr::try_from(addr).map(|addr| addr.with_p2p(node_id)) {
                Ok(Ok(a)) => addrs.push(a),
                Ok(Err(a)) => {
                    debug!("Unable to parse multiaddr: {a} is not compatible with {node_id}")
                }
                Err(e) => debug!("Unable to parse multiaddr: {e}"),
            };
        }

        Ok(KadPeer {
            node_id,
            multiaddrs: addrs,
            connection_ty: peer.connection.into(),
        })
    }
}

impl From<KadPeer> for proto::Peer {
    fn from(peer: KadPeer) -> Self {
        proto::Peer {
            id: peer.node_id.to_bytes(),
            addrs: peer.multiaddrs.into_iter().map(|a| a.to_vec()).collect(),
            connection: peer.connection_ty.into(),
        }
    }
}

/// Configuration for a Kademlia connection upgrade. When applied to a connection, turns this
/// connection into a `Stream + Sink` whose items are of type `KadRequestMsg` and `KadResponseMsg`.
// TODO: if, as suspected, we can confirm with Protocol Labs that each open Kademlia substream does
//       only one request, then we can change the output of the `InboundUpgrade` and
//       `OutboundUpgrade` to be just a single message
#[derive(Debug, Clone)]
pub struct ProtocolConfig {
    protocol_names: Vec<StreamProtocol>,
    /// Maximum allowed size of a packet.
    max_packet_size: usize,
}

impl ProtocolConfig {
    /// Builds a new `ProtocolConfig` with the given protocol name.
    pub fn new(protocol_name: StreamProtocol) -> Self {
        ProtocolConfig {
            protocol_names: vec![protocol_name],
            max_packet_size: DEFAULT_MAX_PACKET_SIZE,
        }
    }

    /// Returns the configured protocol name.
    pub fn protocol_names(&self) -> &[StreamProtocol] {
        &self.protocol_names
    }

    /// Modifies the maximum allowed size of a single Kademlia packet.
    pub fn set_max_packet_size(&mut self, size: usize) {
        self.max_packet_size = size;
    }
}

impl UpgradeInfo for ProtocolConfig {
    type Info = StreamProtocol;
    type InfoIter = std::vec::IntoIter<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.protocol_names.clone().into_iter()
    }
}

/// Codec for Kademlia inbound and outbound message framing.
pub struct Codec<A, B> {
    codec: quick_protobuf_codec::Codec<proto::Message>,
    __phantom: PhantomData<(A, B)>,
}
impl<A, B> Codec<A, B> {
    fn new(max_packet_size: usize) -> Self {
        Codec {
            codec: quick_protobuf_codec::Codec::new(max_packet_size),
            __phantom: PhantomData,
        }
    }
}

impl<A: Into<proto::Message>, B> Encoder for Codec<A, B> {
    type Error = io::Error;
    type Item<'a> = A;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        Ok(self.codec.encode(item.into(), dst)?)
    }
}
impl<A, B: TryFrom<proto::Message, Error = io::Error>> Decoder for Codec<A, B> {
    type Error = io::Error;
    type Item = B;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.codec.decode(src)?.map(B::try_from).transpose()
    }
}

/// Sink of responses and stream of requests.
pub(crate) type KadInStreamSink<S> = Framed<S, Codec<KadResponseMsg, KadRequestMsg>>;
/// Sink of requests and stream of responses.
pub(crate) type KadOutStreamSink<S> = Framed<S, Codec<KadRequestMsg, KadResponseMsg>>;

impl<C> InboundUpgrade<C> for ProtocolConfig
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    type Output = KadInStreamSink<C>;
    type Future = future::Ready<Result<Self::Output, io::Error>>;
    type Error = io::Error;

    fn upgrade_inbound(self, incoming: C, _: Self::Info) -> Self::Future {
        let codec = Codec::new(self.max_packet_size);

        future::ok(Framed::new(incoming, codec))
    }
}

impl<C> OutboundUpgrade<C> for ProtocolConfig
where
    C: AsyncRead + AsyncWrite + Unpin,
{
    type Output = KadOutStreamSink<C>;
    type Future = future::Ready<Result<Self::Output, io::Error>>;
    type Error = io::Error;

    fn upgrade_outbound(self, incoming: C, _: Self::Info) -> Self::Future {
        let codec = Codec::new(self.max_packet_size);

        future::ok(Framed::new(incoming, codec))
    }
}

/// Request that we can send to a peer or that we received from a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KadRequestMsg {
    /// Ping request.
    Ping,

    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNode {
        /// The key for which to locate the closest nodes.
        key: Vec<u8>,
    },

    /// Same as `FindNode`, but should also return the entries of the local providers list for
    /// this key.
    GetProviders {
        /// Identifier being searched.
        key: record::Key,
    },

    /// Indicates that this list of providers is known for this key.
    AddProvider {
        /// Key for which we should add providers.
        key: record::Key,
        /// Known provider for this key.
        provider: KadPeer,
    },

    /// Request to get a value from the dht records.
    GetValue {
        /// The key we are searching for.
        key: record::Key,
    },

    /// Request to put a value into the dht records.
    PutValue { record: Record },
}

/// Response that we can send to a peer or that we received from a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KadResponseMsg {
    /// Ping response.
    Pong,

    /// Response to a `FindNode`.
    FindNode {
        /// Results of the request.
        closer_peers: Vec<KadPeer>,
    },

    /// Response to a `GetProviders`.
    GetProviders {
        /// Nodes closest to the key.
        closer_peers: Vec<KadPeer>,
        /// Known providers for this key.
        provider_peers: Vec<KadPeer>,
    },

    /// Response to a `GetValue`.
    GetValue {
        /// Result that might have been found
        record: Option<Record>,
        /// Nodes closest to the key
        closer_peers: Vec<KadPeer>,
    },

    /// Response to a `PutValue`.
    PutValue {
        /// The key of the record.
        key: record::Key,
        /// Value of the record.
        value: Vec<u8>,
    },
}

impl From<KadRequestMsg> for proto::Message {
    fn from(kad_msg: KadRequestMsg) -> Self {
        req_msg_to_proto(kad_msg)
    }
}
impl From<KadResponseMsg> for proto::Message {
    fn from(kad_msg: KadResponseMsg) -> Self {
        resp_msg_to_proto(kad_msg)
    }
}
impl TryFrom<proto::Message> for KadRequestMsg {
    type Error = io::Error;

    fn try_from(message: proto::Message) -> Result<Self, Self::Error> {
        proto_to_req_msg(message)
    }
}
impl TryFrom<proto::Message> for KadResponseMsg {
    type Error = io::Error;

    fn try_from(message: proto::Message) -> Result<Self, Self::Error> {
        proto_to_resp_msg(message)
    }
}

/// Converts a `KadRequestMsg` into the corresponding protobuf message for sending.
fn req_msg_to_proto(kad_msg: KadRequestMsg) -> proto::Message {
    match kad_msg {
        KadRequestMsg::Ping => proto::Message {
            type_pb: proto::MessageType::PING,
            ..proto::Message::default()
        },
        KadRequestMsg::FindNode { key } => proto::Message {
            type_pb: proto::MessageType::FIND_NODE,
            key,
            clusterLevelRaw: 10,
            ..proto::Message::default()
        },
        KadRequestMsg::GetProviders { key } => proto::Message {
            type_pb: proto::MessageType::GET_PROVIDERS,
            key: key.to_vec(),
            clusterLevelRaw: 10,
            ..proto::Message::default()
        },
        KadRequestMsg::AddProvider { key, provider } => proto::Message {
            type_pb: proto::MessageType::ADD_PROVIDER,
            clusterLevelRaw: 10,
            key: key.to_vec(),
            providerPeers: vec![provider.into()],
            ..proto::Message::default()
        },
        KadRequestMsg::GetValue { key } => proto::Message {
            type_pb: proto::MessageType::GET_VALUE,
            clusterLevelRaw: 10,
            key: key.to_vec(),
            ..proto::Message::default()
        },
        KadRequestMsg::PutValue { record } => proto::Message {
            type_pb: proto::MessageType::PUT_VALUE,
            key: record.key.to_vec(),
            record: Some(record_to_proto(record)),
            ..proto::Message::default()
        },
    }
}

/// Converts a `KadResponseMsg` into the corresponding protobuf message for sending.
fn resp_msg_to_proto(kad_msg: KadResponseMsg) -> proto::Message {
    match kad_msg {
        KadResponseMsg::Pong => proto::Message {
            type_pb: proto::MessageType::PING,
            ..proto::Message::default()
        },
        KadResponseMsg::FindNode { closer_peers } => proto::Message {
            type_pb: proto::MessageType::FIND_NODE,
            clusterLevelRaw: 9,
            closerPeers: closer_peers.into_iter().map(KadPeer::into).collect(),
            ..proto::Message::default()
        },
        KadResponseMsg::GetProviders {
            closer_peers,
            provider_peers,
        } => proto::Message {
            type_pb: proto::MessageType::GET_PROVIDERS,
            clusterLevelRaw: 9,
            closerPeers: closer_peers.into_iter().map(KadPeer::into).collect(),
            providerPeers: provider_peers.into_iter().map(KadPeer::into).collect(),
            ..proto::Message::default()
        },
        KadResponseMsg::GetValue {
            record,
            closer_peers,
        } => proto::Message {
            type_pb: proto::MessageType::GET_VALUE,
            clusterLevelRaw: 9,
            closerPeers: closer_peers.into_iter().map(KadPeer::into).collect(),
            record: record.map(record_to_proto),
            ..proto::Message::default()
        },
        KadResponseMsg::PutValue { key, value } => proto::Message {
            type_pb: proto::MessageType::PUT_VALUE,
            key: key.to_vec(),
            record: Some(proto::Record {
                key: key.to_vec(),
                value,
                ..proto::Record::default()
            }),
            ..proto::Message::default()
        },
    }
}

/// Converts a received protobuf message into a corresponding `KadRequestMsg`.
///
/// Fails if the protobuf message is not a valid and supported Kademlia request message.
fn proto_to_req_msg(message: proto::Message) -> Result<KadRequestMsg, io::Error> {
    match message.type_pb {
        proto::MessageType::PING => Ok(KadRequestMsg::Ping),
        proto::MessageType::PUT_VALUE => {
            let record = record_from_proto(message.record.unwrap_or_default())?;
            Ok(KadRequestMsg::PutValue { record })
        }
        proto::MessageType::GET_VALUE => Ok(KadRequestMsg::GetValue {
            key: record::Key::from(message.key),
        }),
        proto::MessageType::FIND_NODE => Ok(KadRequestMsg::FindNode { key: message.key }),
        proto::MessageType::GET_PROVIDERS => Ok(KadRequestMsg::GetProviders {
            key: record::Key::from(message.key),
        }),
        proto::MessageType::ADD_PROVIDER => {
            // TODO: for now we don't parse the peer properly, so it is possible that we get
            //       parsing errors for peers even when they are valid; we ignore these
            //       errors for now, but ultimately we should just error altogether
            let provider = message
                .providerPeers
                .into_iter()
                .find_map(|peer| KadPeer::try_from(peer).ok());

            if let Some(provider) = provider {
                let key = record::Key::from(message.key);
                Ok(KadRequestMsg::AddProvider { key, provider })
            } else {
                Err(invalid_data("AddProvider message with no valid peer."))
            }
        }
    }
}

/// Converts a received protobuf message into a corresponding `KadResponseMessage`.
///
/// Fails if the protobuf message is not a valid and supported Kademlia response message.
fn proto_to_resp_msg(message: proto::Message) -> Result<KadResponseMsg, io::Error> {
    match message.type_pb {
        proto::MessageType::PING => Ok(KadResponseMsg::Pong),
        proto::MessageType::GET_VALUE => {
            let record = if let Some(r) = message.record {
                Some(record_from_proto(r)?)
            } else {
                None
            };

            let closer_peers = message
                .closerPeers
                .into_iter()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect();

            Ok(KadResponseMsg::GetValue {
                record,
                closer_peers,
            })
        }

        proto::MessageType::FIND_NODE => {
            let closer_peers = message
                .closerPeers
                .into_iter()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect();

            Ok(KadResponseMsg::FindNode { closer_peers })
        }

        proto::MessageType::GET_PROVIDERS => {
            let closer_peers = message
                .closerPeers
                .into_iter()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect();

            let provider_peers = message
                .providerPeers
                .into_iter()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect();

            Ok(KadResponseMsg::GetProviders {
                closer_peers,
                provider_peers,
            })
        }

        proto::MessageType::PUT_VALUE => {
            let key = record::Key::from(message.key);
            let rec = message
                .record
                .ok_or_else(|| invalid_data("received PutValue message with no record"))?;

            Ok(KadResponseMsg::PutValue {
                key,
                value: rec.value,
            })
        }

        proto::MessageType::ADD_PROVIDER => {
            Err(invalid_data("received an unexpected AddProvider message"))
        }
    }
}

fn record_from_proto(record: proto::Record) -> Result<Record, io::Error> {
    let key = record::Key::from(record.key);
    let value = record.value;

    let publisher = if !record.publisher.is_empty() {
        PeerId::from_bytes(&record.publisher)
            .map(Some)
            .map_err(|_| invalid_data("Invalid publisher peer ID."))?
    } else {
        None
    };

    let expires = if record.ttl > 0 {
        Some(Instant::now() + Duration::from_secs(record.ttl as u64))
    } else {
        None
    };

    Ok(Record {
        key,
        value,
        publisher,
        expires,
    })
}

fn record_to_proto(record: Record) -> proto::Record {
    proto::Record {
        key: record.key.to_vec(),
        value: record.value,
        publisher: record.publisher.map(|id| id.to_bytes()).unwrap_or_default(),
        ttl: record
            .expires
            .map(|t| {
                let now = Instant::now();
                if t > now {
                    (t - now).as_secs() as u32
                } else {
                    1 // because 0 means "does not expire"
                }
            })
            .unwrap_or(0),
        timeReceived: String::new(),
    }
}

/// Creates an `io::Error` with `io::ErrorKind::InvalidData`.
fn invalid_data<E>(e: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::InvalidData, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_p2p() {
        let peer_id = PeerId::random();
        let multiaddr = "/ip6/2001:db8::/tcp/1234".parse::<Multiaddr>().unwrap();

        let payload = proto::Peer {
            id: peer_id.to_bytes(),
            addrs: vec![multiaddr.to_vec()],
            connection: proto::ConnectionType::CAN_CONNECT,
        };

        let peer = KadPeer::try_from(payload).unwrap();

        assert_eq!(peer.multiaddrs, vec![multiaddr.with_p2p(peer_id).unwrap()])
    }

    #[test]
    fn skip_invalid_multiaddr() {
        let peer_id = PeerId::random();
        let multiaddr = "/ip6/2001:db8::/tcp/1234".parse::<Multiaddr>().unwrap();

        let valid_multiaddr = multiaddr.clone().with_p2p(peer_id).unwrap();

        let multiaddr_with_incorrect_peer_id = {
            let other_peer_id = PeerId::random();
            assert_ne!(peer_id, other_peer_id);
            multiaddr.with_p2p(other_peer_id).unwrap()
        };

        let invalid_multiaddr = {
            let a = vec![255; 8];
            assert!(Multiaddr::try_from(a.clone()).is_err());
            a
        };

        let payload = proto::Peer {
            id: peer_id.to_bytes(),
            addrs: vec![
                valid_multiaddr.to_vec(),
                multiaddr_with_incorrect_peer_id.to_vec(),
                invalid_multiaddr,
            ],
            connection: proto::ConnectionType::CAN_CONNECT,
        };

        let peer = KadPeer::try_from(payload).unwrap();

        assert_eq!(peer.multiaddrs, vec![valid_multiaddr])
    }

    // // TODO: restore
    // use self::libp2p_tcp::TcpTransport;
    // use self::tokio::runtime::current_thread::Runtime;
    // use futures::{Future, Sink, Stream};
    // use libp2p_core::{PeerId, PublicKey, Transport};
    // use multihash::{encode, Hash};
    // use protocol::{ConnectionType, KadPeer, ProtocolConfig};
    // use std::sync::mpsc;
    // use std::thread;
    //
    // #[test]
    // fn correct_transfer() {
    // We open a server and a client, send a message between the two, and check that they were
    // successfully received.
    //
    // test_one(KadMsg::Ping);
    // test_one(KadMsg::FindNodeReq {
    // key: PeerId::random(),
    // });
    // test_one(KadMsg::FindNodeRes {
    // closer_peers: vec![KadPeer {
    // node_id: PeerId::random(),
    // multiaddrs: vec!["/ip4/100.101.102.103/tcp/20105".parse().unwrap()],
    // connection_ty: ConnectionType::Connected,
    // }],
    // });
    // test_one(KadMsg::GetProvidersReq {
    // key: encode(Hash::SHA2256, &[9, 12, 0, 245, 245, 201, 28, 95]).unwrap(),
    // });
    // test_one(KadMsg::GetProvidersRes {
    // closer_peers: vec![KadPeer {
    // node_id: PeerId::random(),
    // multiaddrs: vec!["/ip4/100.101.102.103/tcp/20105".parse().unwrap()],
    // connection_ty: ConnectionType::Connected,
    // }],
    // provider_peers: vec![KadPeer {
    // node_id: PeerId::random(),
    // multiaddrs: vec!["/ip4/200.201.202.203/tcp/1999".parse().unwrap()],
    // connection_ty: ConnectionType::NotConnected,
    // }],
    // });
    // test_one(KadMsg::AddProvider {
    // key: encode(Hash::SHA2256, &[9, 12, 0, 245, 245, 201, 28, 95]).unwrap(),
    // provider_peer: KadPeer {
    // node_id: PeerId::random(),
    // multiaddrs: vec!["/ip4/9.1.2.3/udp/23".parse().unwrap()],
    // connection_ty: ConnectionType::Connected,
    // },
    // });
    // TODO: all messages
    //
    // fn test_one(msg_server: KadMsg) {
    // let msg_client = msg_server.clone();
    // let (tx, rx) = mpsc::channel();
    //
    // let bg_thread = thread::spawn(move || {
    // let transport = TcpTransport::default().with_upgrade(ProtocolConfig);
    //
    // let (listener, addr) = transport
    // .listen_on( "/ip4/127.0.0.1/tcp/0".parse().unwrap())
    // .unwrap();
    // tx.send(addr).unwrap();
    //
    // let future = listener
    // .into_future()
    // .map_err(|(err, _)| err)
    // .and_then(|(client, _)| client.unwrap().0)
    // .and_then(|proto| proto.into_future().map_err(|(err, _)| err).map(|(v, _)| v))
    // .map(|recv_msg| {
    // assert_eq!(recv_msg.unwrap(), msg_server);
    // ()
    // });
    // let mut rt = Runtime::new().unwrap();
    // let _ = rt.block_on(future).unwrap();
    // });
    //
    // let transport = TcpTransport::default().with_upgrade(ProtocolConfig);
    //
    // let future = transport
    // .dial(rx.recv().unwrap())
    // .unwrap()
    // .and_then(|proto| proto.send(msg_client))
    // .map(|_| ());
    // let mut rt = Runtime::new().unwrap();
    // let _ = rt.block_on(future).unwrap();
    // bg_thread.join().unwrap();
    // }
    // }
}
