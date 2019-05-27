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
//! The connection protocol upgrade is provided by [`KademliaProtocolConfig`], with the
//! request and response types [`KadRequestMsg`] and [`KadResponseMsg`], respectively.
//! The upgrade's output is a `Sink + Stream` of messages. The `Stream` component is used
//! to poll the underlying transport for incoming messages, and the `Sink` component
//! is used to send messages to remote peers.
//!
//! [`KademliaProtocolConfig`]: protocol::KademliaProtocolConfig
//! [`KadRequestMsg`]: protocol::KadRequestMsg
//! [`KadResponseMsg`]: protocol::KadResponseMsg

use bytes::BytesMut;
use codec::UviBytes;
use crate::protobuf_structs::dht as proto;
use crate::record::Record;
use futures::{future::{self, FutureResult}, sink, stream, Sink, Stream};
use libp2p_core::{Multiaddr, PeerId};
use libp2p_core::upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo, Negotiated};
use multihash::Multihash;
use protobuf::{self, Message};
use std::{borrow::Cow, convert::TryFrom};
use std::{io, iter};
use tokio_codec::Framed;
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec;

/// Status of our connection to a node reported by the Kademlia protocol.
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

impl From<proto::Message_ConnectionType> for KadConnectionType {
    #[inline]
    fn from(raw: proto::Message_ConnectionType) -> KadConnectionType {
        use proto::Message_ConnectionType::{
            CAN_CONNECT, CANNOT_CONNECT, CONNECTED, NOT_CONNECTED
        };
        match raw {
            NOT_CONNECTED => KadConnectionType::NotConnected,
            CONNECTED => KadConnectionType::Connected,
            CAN_CONNECT => KadConnectionType::CanConnect,
            CANNOT_CONNECT => KadConnectionType::CannotConnect,
        }
    }
}

impl Into<proto::Message_ConnectionType> for KadConnectionType {
    #[inline]
    fn into(self) -> proto::Message_ConnectionType {
        use proto::Message_ConnectionType::{
            CAN_CONNECT, CANNOT_CONNECT, CONNECTED, NOT_CONNECTED
        };
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
    /// Identifier of the peer.
    pub node_id: PeerId,
    /// The multiaddresses that the sender think can be used in order to reach the peer.
    pub multiaddrs: Vec<Multiaddr>,
    /// How the sender is connected to that remote.
    pub connection_ty: KadConnectionType,
}

// Builds a `KadPeer` from a corresponding protobuf message.
impl TryFrom<&mut proto::Message_Peer> for KadPeer {
    type Error = io::Error;

    fn try_from(peer: &mut proto::Message_Peer) -> Result<KadPeer, Self::Error> {
        // TODO: this is in fact a CID; not sure if this should be handled in `from_bytes` or
        //       as a special case here
        let node_id = PeerId::from_bytes(peer.get_id().to_vec())
            .map_err(|_| invalid_data("invalid peer id"))?;

        let mut addrs = Vec::with_capacity(peer.get_addrs().len());
        for addr in peer.take_addrs().into_iter() {
            let as_ma = Multiaddr::try_from(addr).map_err(invalid_data)?;
            addrs.push(as_ma);
        }
        debug_assert_eq!(addrs.len(), addrs.capacity());

        let connection_ty = peer.get_connection().into();

        Ok(KadPeer {
            node_id,
            multiaddrs: addrs,
            connection_ty
        })
    }
}

impl Into<proto::Message_Peer> for KadPeer {
    fn into(self) -> proto::Message_Peer {
        let mut out = proto::Message_Peer::new();
        out.set_id(self.node_id.into_bytes());
        for addr in self.multiaddrs {
            out.mut_addrs().push(addr.to_vec());
        }
        out.set_connection(self.connection_ty.into());
        out
    }
}

/// Configuration for a Kademlia connection upgrade. When applied to a connection, turns this
/// connection into a `Stream + Sink` whose items are of type `KadRequestMsg` and `KadResponseMsg`.
// TODO: if, as suspected, we can confirm with Protocol Labs that each open Kademlia substream does
//       only one request, then we can change the output of the `InboundUpgrade` and
//       `OutboundUpgrade` to be just a single message
#[derive(Debug, Clone)]
pub struct KademliaProtocolConfig {
    protocol_name: Cow<'static, [u8]>,
}

impl KademliaProtocolConfig {
    /// Modifies the protocol name used on the wire. Can be used to create incompatibilities
    /// between networks on purpose.
    pub fn with_protocol_name(mut self, name: impl Into<Cow<'static, [u8]>>) -> Self {
        self.protocol_name = name.into();
        self
    }
}

impl Default for KademliaProtocolConfig {
    fn default() -> Self {
        KademliaProtocolConfig {
            protocol_name: Cow::Borrowed(b"/ipfs/kad/1.0.0"),
        }
    }
}

impl UpgradeInfo for KademliaProtocolConfig {
    type Info = Cow<'static, [u8]>;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.protocol_name.clone())
    }
}

impl<C> InboundUpgrade<C> for KademliaProtocolConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = KadInStreamSink<Negotiated<C>>;
    type Future = FutureResult<Self::Output, io::Error>;
    type Error = io::Error;

    #[inline]
    fn upgrade_inbound(self, incoming: Negotiated<C>, _: Self::Info) -> Self::Future {
        let mut codec = UviBytes::default();
        codec.set_max_len(4096);

        future::ok(
            Framed::new(incoming, codec)
                .from_err()
                .with::<_, fn(_) -> _, _>(|response| {
                    let proto_struct = resp_msg_to_proto(response);
                    proto_struct.write_to_bytes().map_err(invalid_data)
                })
                .and_then::<fn(_) -> _, _>(|bytes| {
                    let request = protobuf::parse_from_bytes(&bytes)?;
                    proto_to_req_msg(request)
                }),
        )
    }
}

impl<C> OutboundUpgrade<C> for KademliaProtocolConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = KadOutStreamSink<Negotiated<C>>;
    type Future = FutureResult<Self::Output, io::Error>;
    type Error = io::Error;

    #[inline]
    fn upgrade_outbound(self, incoming: Negotiated<C>, _: Self::Info) -> Self::Future {
        let mut codec = UviBytes::default();
        codec.set_max_len(4096);

        future::ok(
            Framed::new(incoming, codec)
                .from_err()
                .with::<_, fn(_) -> _, _>(|request| {
                    let proto_struct = req_msg_to_proto(request);
                    proto_struct.write_to_bytes().map_err(invalid_data)
                })
                .and_then::<fn(_) -> _, _>(|bytes| {
                    let response = protobuf::parse_from_bytes(&bytes)?;
                    proto_to_resp_msg(response)
                }),
        )
    }
}

/// Sink of responses and stream of requests.
pub type KadInStreamSink<S> = KadStreamSink<S, KadResponseMsg, KadRequestMsg>;

/// Sink of requests and stream of responses.
pub type KadOutStreamSink<S> = KadStreamSink<S, KadRequestMsg, KadResponseMsg>;

pub type KadStreamSink<S, A, B> = stream::AndThen<
    sink::With<
        stream::FromErr<Framed<S, UviBytes<Vec<u8>>>, io::Error>,
        A,
        fn(A) -> Result<Vec<u8>, io::Error>,
        Result<Vec<u8>, io::Error>,
    >,
    fn(BytesMut) -> Result<B, io::Error>,
    Result<B, io::Error>,
>;

/// Request that we can send to a peer or that we received from a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KadRequestMsg {
    /// Ping request.
    Ping,

    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNode {
        /// The key for which to locate the closest nodes.
        key: Multihash,
    },

    /// Same as `FindNode`, but should also return the entries of the local providers list for
    /// this key.
    GetProviders {
        /// Identifier being searched.
        key: Multihash,
    },

    /// Indicates that this list of providers is known for this key.
    AddProvider {
        /// Key for which we should add providers.
        key: Multihash,
        /// Known provider for this key.
        provider_peer: KadPeer,
    },

    /// Request to get a value from the dht records.
    GetValue {
        /// The key we are searching for.
        key: Multihash,
    },

    /// Request to put a value into the dht records.
    PutValue {
        /// The key of the record.
        key: Multihash,
        /// The value of the record.
        value: Vec<u8>,
    }
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
        result: Option<Record>,
        /// Nodes closest to the key
        closer_peers: Vec<KadPeer>,
    },

    /// Response to a `PutValue`.
    PutValue {
        /// The key of the record.
        key: Multihash,
        /// Value of the record.
        value: Vec<u8>,
    },
}

/// Converts a `KadRequestMsg` into the corresponding protobuf message for sending.
fn req_msg_to_proto(kad_msg: KadRequestMsg) -> proto::Message {
    match kad_msg {
        KadRequestMsg::Ping => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::PING);
            msg
        }
        KadRequestMsg::FindNode { key } => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::FIND_NODE);
            msg.set_key(key.into_bytes());
            msg.set_clusterLevelRaw(10);
            msg
        }
        KadRequestMsg::GetProviders { key } => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::GET_PROVIDERS);
            msg.set_key(key.into_bytes());
            msg.set_clusterLevelRaw(10);
            msg
        }
        KadRequestMsg::AddProvider { key, provider_peer } => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::ADD_PROVIDER);
            msg.set_clusterLevelRaw(10);
            msg.set_key(key.into_bytes());
            msg.mut_providerPeers().push(provider_peer.into());
            msg
        }
        KadRequestMsg::GetValue { key } => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::GET_VALUE);
            msg.set_clusterLevelRaw(10);
            msg.set_key(key.into_bytes());

            msg
        }
        KadRequestMsg::PutValue { key, value} => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::PUT_VALUE);
            let mut record = proto::Record::new();
            record.set_value(value);
            record.set_key(key.into_bytes());

            msg.set_record(record);
            msg
        }
    }
}

/// Converts a `KadResponseMsg` into the corresponding protobuf message for sending.
fn resp_msg_to_proto(kad_msg: KadResponseMsg) -> proto::Message {
    match kad_msg {
        KadResponseMsg::Pong => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::PING);
            msg
        }
        KadResponseMsg::FindNode { closer_peers } => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::FIND_NODE);
            msg.set_clusterLevelRaw(9);
            for peer in closer_peers {
                msg.mut_closerPeers().push(peer.into());
            }
            msg
        }
        KadResponseMsg::GetProviders {
            closer_peers,
            provider_peers,
        } => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::GET_PROVIDERS);
            msg.set_clusterLevelRaw(9);
            for peer in closer_peers {
                msg.mut_closerPeers().push(peer.into());
            }
            for peer in provider_peers {
                msg.mut_providerPeers().push(peer.into());
            }
            msg
        }
        KadResponseMsg::GetValue {
            result,
            closer_peers,
        } => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::GET_VALUE);
            msg.set_clusterLevelRaw(9);
            for peer in closer_peers {
                msg.mut_closerPeers().push(peer.into());
            }

            if let Some(Record{ key, value }) = result {
                let mut record = proto::Record::new();
                record.set_key(key.into_bytes());
                record.set_value(value);
                msg.set_record(record);
            }

            msg
        }
        KadResponseMsg::PutValue {
            key,
            value,
        } => {
            let mut msg = proto::Message::new();
            msg.set_field_type(proto::Message_MessageType::PUT_VALUE);
            msg.set_key(key.clone().into_bytes());

            let mut record = proto::Record::new();
            record.set_key(key.into_bytes());
            record.set_value(value);
            msg.set_record(record);

            msg
        }
    }
}

/// Converts a received protobuf message into a corresponding `KadRequestMsg`.
///
/// Fails if the protobuf message is not a valid and supported Kademlia request message.
fn proto_to_req_msg(mut message: proto::Message) -> Result<KadRequestMsg, io::Error> {
    match message.get_field_type() {
        proto::Message_MessageType::PING => Ok(KadRequestMsg::Ping),

        proto::Message_MessageType::PUT_VALUE => {
            let record = message.mut_record();
            let key = Multihash::from_bytes(record.take_key()).map_err(invalid_data)?;
            Ok(KadRequestMsg::PutValue { key, value: record.take_value() })
        }

        proto::Message_MessageType::GET_VALUE => {
            let key = Multihash::from_bytes(message.take_key()).map_err(invalid_data)?;
            Ok(KadRequestMsg::GetValue { key })
        }

        proto::Message_MessageType::FIND_NODE => {
            let key = Multihash::from_bytes(message.take_key())
                .map_err(|_| invalid_data("Invalid key in FIND_NODE"))?;
            Ok(KadRequestMsg::FindNode { key })
        }

        proto::Message_MessageType::GET_PROVIDERS => {
            let key = Multihash::from_bytes(message.take_key()).map_err(invalid_data)?;
            Ok(KadRequestMsg::GetProviders { key })
        }

        proto::Message_MessageType::ADD_PROVIDER => {
            // TODO: for now we don't parse the peer properly, so it is possible that we get
            //       parsing errors for peers even when they are valid; we ignore these
            //       errors for now, but ultimately we should just error altogether
            let provider_peer = message
                .mut_providerPeers()
                .iter_mut()
                .find_map(|peer| KadPeer::try_from(peer).ok());

            if let Some(provider_peer) = provider_peer {
                let key = Multihash::from_bytes(message.take_key()).map_err(invalid_data)?;
                Ok(KadRequestMsg::AddProvider { key, provider_peer })
            } else {
                Err(invalid_data("ADD_PROVIDER message with no valid peer."))
            }
        }
    }
}

/// Converts a received protobuf message into a corresponding `KadResponseMessage`.
///
/// Fails if the protobuf message is not a valid and supported Kademlia response message.
fn proto_to_resp_msg(mut message: proto::Message) -> Result<KadResponseMsg, io::Error> {
    match message.get_field_type() {
        proto::Message_MessageType::PING => Ok(KadResponseMsg::Pong),

        proto::Message_MessageType::GET_VALUE => {
            let result = match message.has_record() {
                true => {
                    let mut record = message.take_record();
                    let key = Multihash::from_bytes(record.take_key()).map_err(invalid_data)?;
                    Some(Record { key, value: record.take_value() })
                }
                false => None,
            };

            let closer_peers = message
                .mut_closerPeers()
                .iter_mut()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect::<Vec<_>>();

            Ok(KadResponseMsg::GetValue { result, closer_peers })
        },

        proto::Message_MessageType::FIND_NODE => {
            let closer_peers = message
                .mut_closerPeers()
                .iter_mut()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect::<Vec<_>>();

            Ok(KadResponseMsg::FindNode { closer_peers })
        }

        proto::Message_MessageType::GET_PROVIDERS => {
            let closer_peers = message
                .mut_closerPeers()
                .iter_mut()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect::<Vec<_>>();

            let provider_peers = message
                .mut_providerPeers()
                .iter_mut()
                .filter_map(|peer| KadPeer::try_from(peer).ok())
                .collect::<Vec<_>>();

            Ok(KadResponseMsg::GetProviders {
                closer_peers,
                provider_peers,
            })
        }

        proto::Message_MessageType::PUT_VALUE => {
            let key = Multihash::from_bytes(message.take_key()).map_err(invalid_data)?;
            if !message.has_record() {
                return Err(invalid_data("received PUT_VALUE message with no record"));
            }

            let mut record = message.take_record();
            Ok(KadResponseMsg::PutValue {
                key,
                value: record.take_value(),
            })
        }

        proto::Message_MessageType::ADD_PROVIDER =>
            Err(invalid_data("received an unexpected ADD_PROVIDER message"))
    }
}

/// Creates an `io::Error` with `io::ErrorKind::InvalidData`.
fn invalid_data<E>(e: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>
{
    io::Error::new(io::ErrorKind::InvalidData, e)
}

#[cfg(test)]
mod tests {

    /*// TODO: restore
    use self::libp2p_tcp::TcpConfig;
    use self::tokio::runtime::current_thread::Runtime;
    use futures::{Future, Sink, Stream};
    use libp2p_core::{PeerId, PublicKey, Transport};
    use multihash::{encode, Hash};
    use protocol::{KadConnectionType, KadPeer, KademliaProtocolConfig};
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn correct_transfer() {
        // We open a server and a client, send a message between the two, and check that they were
        // successfully received.

        test_one(KadMsg::Ping);
        test_one(KadMsg::FindNodeReq {
            key: PeerId::random(),
        });
        test_one(KadMsg::FindNodeRes {
            closer_peers: vec![KadPeer {
                node_id: PeerId::random(),
                multiaddrs: vec!["/ip4/100.101.102.103/tcp/20105".parse().unwrap()],
                connection_ty: KadConnectionType::Connected,
            }],
        });
        test_one(KadMsg::GetProvidersReq {
            key: encode(Hash::SHA2256, &[9, 12, 0, 245, 245, 201, 28, 95]).unwrap(),
        });
        test_one(KadMsg::GetProvidersRes {
            closer_peers: vec![KadPeer {
                node_id: PeerId::random(),
                multiaddrs: vec!["/ip4/100.101.102.103/tcp/20105".parse().unwrap()],
                connection_ty: KadConnectionType::Connected,
            }],
            provider_peers: vec![KadPeer {
                node_id: PeerId::random(),
                multiaddrs: vec!["/ip4/200.201.202.203/tcp/1999".parse().unwrap()],
                connection_ty: KadConnectionType::NotConnected,
            }],
        });
        test_one(KadMsg::AddProvider {
            key: encode(Hash::SHA2256, &[9, 12, 0, 245, 245, 201, 28, 95]).unwrap(),
            provider_peer: KadPeer {
                node_id: PeerId::random(),
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
                .unwrap()
                .and_then(|proto| proto.send(msg_client))
                .map(|_| ());
            let mut rt = Runtime::new().unwrap();
            let _ = rt.block_on(future).unwrap();
            bg_thread.join().unwrap();
        }
    }*/
}
