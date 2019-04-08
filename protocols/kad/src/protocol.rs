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

//! Provides the `KadRequestMsg` and `KadResponseMsg` enums of all the possible messages
//! transmitted with the Kademlia protocol, and the `KademliaProtocolConfig` connection upgrade.
//!
//! The upgrade's output a `Sink + Stream` of messages.
//!
//! The `Stream` component is used to poll the underlying transport, and the `Sink` component is
//! used to send messages.

use bytes::BytesMut;
use crate::protobuf_structs;
use futures::{future, sink, stream, Sink, Stream};
use libp2p_core::{InboundUpgrade, Multiaddr, OutboundUpgrade, PeerId, UpgradeInfo, upgrade::Negotiated};
use multihash::Multihash;
use protobuf::{self, Message};
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
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

impl From<protobuf_structs::dht::Message_ConnectionType> for KadConnectionType {
    #[inline]
    fn from(raw: protobuf_structs::dht::Message_ConnectionType) -> KadConnectionType {
        use crate::protobuf_structs::dht::Message_ConnectionType::{
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

impl Into<protobuf_structs::dht::Message_ConnectionType> for KadConnectionType {
    #[inline]
    fn into(self) -> protobuf_structs::dht::Message_ConnectionType {
        use crate::protobuf_structs::dht::Message_ConnectionType::{
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

impl KadPeer {
    // Builds a `KadPeer` from its raw protobuf equivalent.
    // TODO: use TryFrom once stable
    fn from_peer(peer: &mut protobuf_structs::dht::Message_Peer) -> Result<KadPeer, IoError> {
        // TODO: this is in fact a CID; not sure if this should be handled in `from_bytes` or
        //       as a special case here
        let node_id = PeerId::from_bytes(peer.get_id().to_vec())
            .map_err(|_| IoError::new(IoErrorKind::InvalidData, "invalid peer id"))?;

        let mut addrs = Vec::with_capacity(peer.get_addrs().len());
        for addr in peer.take_addrs().into_iter() {
            let as_ma = Multiaddr::try_from_vec(addr)
                .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))?;
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

impl Into<protobuf_structs::dht::Message_Peer> for KadPeer {
    fn into(self) -> protobuf_structs::dht::Message_Peer {
        let mut out = protobuf_structs::dht::Message_Peer::new();
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
#[derive(Debug, Default, Copy, Clone)]
pub struct KademliaProtocolConfig;

impl UpgradeInfo for KademliaProtocolConfig {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    #[inline]
    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/ipfs/kad/1.0.0")
    }
}

impl<C> InboundUpgrade<C> for KademliaProtocolConfig
where
    C: AsyncRead + AsyncWrite,
{
    type Output = KadInStreamSink<Negotiated<C>>;
    type Future = future::FutureResult<Self::Output, IoError>;
    type Error = IoError;

    #[inline]
    fn upgrade_inbound(self, incoming: Negotiated<C>, _: Self::Info) -> Self::Future {
        let mut codec = codec::UviBytes::default();
        codec.set_max_len(4096);

        future::ok(
            Framed::new(incoming, codec)
                .from_err::<IoError>()
                .with::<_, fn(_) -> _, _>(|response| -> Result<_, IoError> {
                    let proto_struct = resp_msg_to_proto(response);
                    proto_struct.write_to_bytes()
                        .map_err(|err| IoError::new(IoErrorKind::InvalidData, err.to_string()))
                })
                .and_then::<fn(_) -> _, _>(|bytes: BytesMut| {
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
    type Future = future::FutureResult<Self::Output, IoError>;
    type Error = IoError;

    #[inline]
    fn upgrade_outbound(self, incoming: Negotiated<C>, _: Self::Info) -> Self::Future {
        let mut codec = codec::UviBytes::default();
        codec.set_max_len(4096);

        future::ok(
            Framed::new(incoming, codec)
                .from_err::<IoError>()
                .with::<_, fn(_) -> _, _>(|request| -> Result<_, IoError> {
                    let proto_struct = req_msg_to_proto(request);
                    match proto_struct.write_to_bytes() {
                        Ok(msg) => Ok(msg),
                        Err(err) => Err(IoError::new(IoErrorKind::Other, err.to_string())),
                    }
                })
                .and_then::<fn(_) -> _, _>(|bytes: BytesMut| {
                    let response = protobuf::parse_from_bytes(&bytes)?;
                    proto_to_resp_msg(response)
                }),
        )
    }
}

/// Sink of responses and stream of requests.
pub type KadInStreamSink<S> = stream::AndThen<
    sink::With<
        stream::FromErr<Framed<S, codec::UviBytes<Vec<u8>>>, IoError>,
        KadResponseMsg,
        fn(KadResponseMsg) -> Result<Vec<u8>, IoError>,
        Result<Vec<u8>, IoError>,
    >,
    fn(BytesMut) -> Result<KadRequestMsg, IoError>,
    Result<KadRequestMsg, IoError>,
>;

/// Sink of requests and stream of responses.
pub type KadOutStreamSink<S> = stream::AndThen<
    sink::With<
        stream::FromErr<Framed<S, codec::UviBytes<Vec<u8>>>, IoError>,
        KadRequestMsg,
        fn(KadRequestMsg) -> Result<Vec<u8>, IoError>,
        Result<Vec<u8>, IoError>,
    >,
    fn(BytesMut) -> Result<KadResponseMsg, IoError>,
    Result<KadResponseMsg, IoError>,
>;

/// Request that we can send to a peer or that we received from a peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KadRequestMsg {
    /// Ping request.
    Ping,

    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNode {
        /// Identifier of the node.
        key: PeerId,
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
}

// Turns a type-safe Kadmelia message into the corresponding raw protobuf message.
fn req_msg_to_proto(kad_msg: KadRequestMsg) -> protobuf_structs::dht::Message {
    match kad_msg {
        KadRequestMsg::Ping => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::PING);
            msg
        }
        KadRequestMsg::FindNode { key } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::FIND_NODE);
            msg.set_key(key.into_bytes());
            msg.set_clusterLevelRaw(10);
            msg
        }
        KadRequestMsg::GetProviders { key } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::GET_PROVIDERS);
            msg.set_key(key.into_bytes());
            msg.set_clusterLevelRaw(10);
            msg
        }
        KadRequestMsg::AddProvider { key, provider_peer } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::ADD_PROVIDER);
            msg.set_clusterLevelRaw(10);
            msg.set_key(key.into_bytes());
            msg.mut_providerPeers().push(provider_peer.into());
            msg
        }
    }
}

// Turns a type-safe Kadmelia message into the corresponding raw protobuf message.
fn resp_msg_to_proto(kad_msg: KadResponseMsg) -> protobuf_structs::dht::Message {
    match kad_msg {
        KadResponseMsg::Pong => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::PING);
            msg
        }
        KadResponseMsg::FindNode { closer_peers } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::FIND_NODE);
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
    }
}

/// Turns a raw Kademlia message into a type-safe message.
fn proto_to_req_msg(mut message: protobuf_structs::dht::Message) -> Result<KadRequestMsg, IoError> {
    match message.get_field_type() {
        protobuf_structs::dht::Message_MessageType::PING => Ok(KadRequestMsg::Ping),

        protobuf_structs::dht::Message_MessageType::PUT_VALUE => {
            Err(IoError::new(
                IoErrorKind::InvalidData,
                "received a PUT_VALUE message, but this is not supported by rust-libp2p yet",
            ))
        }

        protobuf_structs::dht::Message_MessageType::GET_VALUE => {
            Err(IoError::new(
                IoErrorKind::InvalidData,
                "received a GET_VALUE message, but this is not supported by rust-libp2p yet",
            ))
        }

        protobuf_structs::dht::Message_MessageType::FIND_NODE => {
            let key = PeerId::from_bytes(message.take_key()).map_err(|_| {
                IoError::new(IoErrorKind::InvalidData, "invalid peer id in FIND_NODE")
            })?;
            Ok(KadRequestMsg::FindNode { key })
        }

        protobuf_structs::dht::Message_MessageType::GET_PROVIDERS => {
            let key = Multihash::from_bytes(message.take_key())
                .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))?;
            Ok(KadRequestMsg::GetProviders { key })
        }

        protobuf_structs::dht::Message_MessageType::ADD_PROVIDER => {
            // TODO: for now we don't parse the peer properly, so it is possible that we get
            //       parsing errors for peers even when they are valid; we ignore these
            //       errors for now, but ultimately we should just error altogether
            let provider_peer = message
                .mut_providerPeers()
                .iter_mut()
                .filter_map(|peer| KadPeer::from_peer(peer).ok())
                .next();

            if let Some(provider_peer) = provider_peer {
                let key = Multihash::from_bytes(message.take_key())
                    .map_err(|err| IoError::new(IoErrorKind::InvalidData, err))?;
                Ok(KadRequestMsg::AddProvider { key, provider_peer })
            } else {
                Err(IoError::new(
                    IoErrorKind::InvalidData,
                    "received an ADD_PROVIDER message with no valid peer",
                ))
            }
        }
    }
}

/// Turns a raw Kademlia message into a type-safe message.
fn proto_to_resp_msg(
    mut message: protobuf_structs::dht::Message,
) -> Result<KadResponseMsg, IoError> {
    match message.get_field_type() {
        protobuf_structs::dht::Message_MessageType::PING => Ok(KadResponseMsg::Pong),

        protobuf_structs::dht::Message_MessageType::GET_VALUE => {
            Err(IoError::new(
                IoErrorKind::InvalidData,
                "received a GET_VALUE message, but this is not supported by rust-libp2p yet",
            ))
        }

        protobuf_structs::dht::Message_MessageType::FIND_NODE => {
            let closer_peers = message
                .mut_closerPeers()
                .iter_mut()
                .filter_map(|peer| KadPeer::from_peer(peer).ok())
                .collect::<Vec<_>>();

            Ok(KadResponseMsg::FindNode { closer_peers })
        }

        protobuf_structs::dht::Message_MessageType::GET_PROVIDERS => {
            let closer_peers = message
                .mut_closerPeers()
                .iter_mut()
                .filter_map(|peer| KadPeer::from_peer(peer).ok())
                .collect::<Vec<_>>();
            let provider_peers = message
                .mut_providerPeers()
                .iter_mut()
                .filter_map(|peer| KadPeer::from_peer(peer).ok())
                .collect::<Vec<_>>();

            Ok(KadResponseMsg::GetProviders {
                closer_peers,
                provider_peers,
            })
        }

        protobuf_structs::dht::Message_MessageType::PUT_VALUE => Err(IoError::new(
            IoErrorKind::InvalidData,
            "received an unexpected PUT_VALUE message",
        )),

        protobuf_structs::dht::Message_MessageType::ADD_PROVIDER => Err(IoError::new(
            IoErrorKind::InvalidData,
            "received an unexpected ADD_PROVIDER message",
        )),
    }
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
