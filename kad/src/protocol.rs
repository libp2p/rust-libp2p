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

use bytes::Bytes;
use futures::future;
use futures::{Sink, Stream};
use libp2p_peerstore::PeerId;
use libp2p_swarm::{ConnectionUpgrade, Endpoint, Multiaddr};
use protobuf::{self, Message};
use protobuf_structs;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use tokio_io::{AsyncRead, AsyncWrite};
use varint::VarintCodec;

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

impl From<protobuf_structs::dht::Message_ConnectionType> for ConnectionType {
    #[inline]
    fn from(raw: protobuf_structs::dht::Message_ConnectionType) -> ConnectionType {
        use protobuf_structs::dht::Message_ConnectionType::*;
        match raw {
            NOT_CONNECTED => ConnectionType::NotConnected,
            CONNECTED => ConnectionType::Connected,
            CAN_CONNECT => ConnectionType::CanConnect,
            CANNOT_CONNECT => ConnectionType::CannotConnect,
        }
    }
}

impl Into<protobuf_structs::dht::Message_ConnectionType> for ConnectionType {
    #[inline]
    fn into(self) -> protobuf_structs::dht::Message_ConnectionType {
        use protobuf_structs::dht::Message_ConnectionType::*;
        match self {
            ConnectionType::NotConnected => NOT_CONNECTED,
            ConnectionType::Connected => CONNECTED,
            ConnectionType::CanConnect => CAN_CONNECT,
            ConnectionType::CannotConnect => CANNOT_CONNECT,
        }
    }
}

/// Information about a peer, as known by the sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub node_id: PeerId,
    /// The multiaddresses that are known for that peer.
    pub multiaddrs: Vec<Multiaddr>,
    pub connection_ty: ConnectionType,
}

impl<'a> From<&'a mut protobuf_structs::dht::Message_Peer> for Peer {
    fn from(peer: &'a mut protobuf_structs::dht::Message_Peer) -> Peer {
        let node_id = PeerId::from_bytes(peer.get_id().to_vec()).unwrap(); // TODO: don't unwrap
        let addrs = peer.take_addrs()
			.into_iter()
			.map(|a| Multiaddr::from_bytes(a).unwrap())		// TODO: don't unwrap
			.collect();
        let connection_ty = peer.get_connection().into();

        Peer {
            node_id: node_id,
            multiaddrs: addrs,
            connection_ty: connection_ty,
        }
    }
}

impl Into<protobuf_structs::dht::Message_Peer> for Peer {
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
    type Output =
        Box<KadStreamSink<Item = KadMsg, Error = IoError, SinkItem = KadMsg, SinkError = IoError>>;
    type Future = future::FutureResult<Self::Output, IoError>;
    type NamesIter = iter::Once<(Bytes, ())>;
    type UpgradeIdentifier = ();

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        iter::once(("/ipfs/kad/1.0.0".into(), ()))
    }

    #[inline]
    fn upgrade(self, incoming: C, _: (), _: Endpoint, _: &Multiaddr) -> Self::Future {
        future::ok(kademlia_protocol(incoming))
    }
}

// Upgrades a socket to use the Kademlia protocol.
fn kademlia_protocol<'a, S>(
    socket: S,
) -> Box<KadStreamSink<Item = KadMsg, Error = IoError, SinkItem = KadMsg, SinkError = IoError> + 'a>
where
    S: AsyncRead + AsyncWrite + 'a,
{
    let wrapped = socket
        .framed(VarintCodec::default())
        .from_err::<IoError>()
        .with(|request| -> Result<_, IoError> {
            let proto_struct = msg_to_proto(request);
            Ok(proto_struct.write_to_bytes().unwrap()) // TODO: error?
        })
        .and_then(|bytes| {
            let response = protobuf::parse_from_bytes(&bytes)?;
            proto_to_msg(response)
        });

    Box::new(wrapped)
}

/// Custom trait that derives `Sink` and `Stream`, so that we can box it.
pub trait KadStreamSink:
    Stream<Item = KadMsg, Error = IoError> + Sink<SinkItem = KadMsg, SinkError = IoError>
{
}
impl<T> KadStreamSink for T
where
    T: Stream<Item = KadMsg, Error = IoError> + Sink<SinkItem = KadMsg, SinkError = IoError>,
{
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
        key: Vec<u8>,
        /// The record itself.
        record: (), //record: protobuf_structs::record::Record, // TODO: no
    },
    GetValueReq {
        /// Identifier of the record.
        key: Vec<u8>,
    },
    GetValueRes {
        /// Identifier of the returned record.
        key: Vec<u8>,
        record: (), //record: Option<protobuf_structs::record::Record>, // TODO: no
        closer_peers: Vec<Peer>,
    },
    /// Request for the list of nodes whose IDs are the closest to `key`. The number of nodes
    /// returned is not specified, but should be around 20.
    FindNodeReq {
        /// Identifier of the node.
        key: Vec<u8>,
    },
    /// Response to a `FindNodeReq`.
    FindNodeRes {
        /// Results of the request.
        closer_peers: Vec<Peer>,
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
            msg.set_key(key);
            //msg.set_record(record);		// TODO:
            msg
        }
        KadMsg::GetValueReq { key } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::GET_VALUE);
            msg.set_key(key);
            msg.set_clusterLevelRaw(10);
            msg
        }
        KadMsg::GetValueRes { .. } => unimplemented!(),
        KadMsg::FindNodeReq { key } => {
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::FIND_NODE);
            msg.set_key(key);
            msg.set_clusterLevelRaw(10);
            msg
        }
        KadMsg::FindNodeRes { closer_peers } => {
            // TODO: if empty, the remote will think it's a request
            assert!(!closer_peers.is_empty());
            let mut msg = protobuf_structs::dht::Message::new();
            msg.set_field_type(protobuf_structs::dht::Message_MessageType::FIND_NODE);
            msg.set_clusterLevelRaw(9);
            for peer in closer_peers {
                msg.mut_closerPeers().push(peer.into());
            }
            msg
        }
    }
}

/// Turns a raw Kademlia message into a type-safe message.
fn proto_to_msg(mut message: protobuf_structs::dht::Message) -> Result<KadMsg, IoError> {
    match message.get_field_type() {
        protobuf_structs::dht::Message_MessageType::PING => Ok(KadMsg::Ping),

        protobuf_structs::dht::Message_MessageType::PUT_VALUE => {
            let key = message.take_key();
            let _record = message.take_record();
            Ok(KadMsg::PutValue {
                key: key,
                record: (),
            })
        }

        protobuf_structs::dht::Message_MessageType::GET_VALUE => {
            let key = message.take_key();
            Ok(KadMsg::GetValueReq { key: key })
        }

        protobuf_structs::dht::Message_MessageType::FIND_NODE => {
            if message.get_closerPeers().is_empty() {
                Ok(KadMsg::FindNodeReq {
                    key: message.take_key(),
                })
            } else {
                Ok(KadMsg::FindNodeRes {
                    closer_peers: message
                        .mut_closerPeers()
                        .iter_mut()
                        .map(|peer| peer.into())
                        .collect(),
                })
            }
        }

        protobuf_structs::dht::Message_MessageType::GET_PROVIDERS
        | protobuf_structs::dht::Message_MessageType::ADD_PROVIDER => {
            // These messages don't seem to be used in the protocol in practice, so if we receive
            // them we suppose that it's a mistake in the protocol usage.
            Err(IoError::new(
                IoErrorKind::InvalidData,
                "received an unsupported kad message type",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate libp2p_tcp_transport;
    extern crate tokio_core;

    use self::libp2p_tcp_transport::TcpConfig;
    use self::tokio_core::reactor::Core;
    use futures::{Future, Sink, Stream};
    use libp2p_peerstore::PeerId;
    use libp2p_swarm::Transport;
    use protocol::{ConnectionType, KadMsg, KademliaProtocolConfig, Peer};
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn correct_transfer() {
        // We open a server and a client, send a message between the two, and check that they were
        // successfully received.

        test_one(KadMsg::Ping);
        test_one(KadMsg::PutValue {
            key: vec![1, 2, 3, 4],
            record: (),
        });
        test_one(KadMsg::GetValueReq {
            key: vec![10, 11, 12],
        });
        test_one(KadMsg::FindNodeReq {
            key: vec![9, 12, 0, 245, 245, 201, 28, 95],
        });
        test_one(KadMsg::FindNodeRes {
            closer_peers: vec![
                Peer {
                    node_id: PeerId::from_public_key(&[93, 80, 12, 250]),
                    multiaddrs: vec!["/ip4/100.101.102.103/tcp/20105".parse().unwrap()],
                    connection_ty: ConnectionType::Connected,
                },
            ],
        });
        // TODO: all messages

        fn test_one(msg_server: KadMsg) {
            let msg_client = msg_server.clone();
            let (tx, rx) = mpsc::channel();

            let bg_thread = thread::spawn(move || {
                let mut core = Core::new().unwrap();
                let transport = TcpConfig::new(core.handle()).with_upgrade(KademliaProtocolConfig);

                let (listener, addr) = transport
                    .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
                    .unwrap();
                tx.send(addr).unwrap();

                let future = listener
                    .into_future()
                    .map_err(|(err, _)| err)
                    .and_then(|(client, _)| client.unwrap().map(|v| v.0))
                    .and_then(|proto| proto.into_future().map_err(|(err, _)| err).map(|(v, _)| v))
                    .map(|recv_msg| {
                        assert_eq!(recv_msg.unwrap(), msg_server);
                        ()
                    });

                let _ = core.run(future).unwrap();
            });

            let mut core = Core::new().unwrap();
            let transport = TcpConfig::new(core.handle()).with_upgrade(KademliaProtocolConfig);

            let future = transport
                .dial(rx.recv().unwrap())
                .unwrap_or_else(|_| panic!())
                .and_then(|proto| proto.0.send(msg_client))
                .map(|_| ());

            let _ = core.run(future).unwrap();
            bg_thread.join().unwrap();
        }
    }
}
