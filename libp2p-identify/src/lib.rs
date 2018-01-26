// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Implementation of the `/ipfs/id/1.0.0` protocol. Allows a node A to query another node B which
//! information B knows about A. Also includes the addresses B is listening on.
//!
//! When two nodes connect to each other, the listening half sends a message to the dialing half,
//! indicating the information, and then the protocol stops.
//!
//! # Usage
//!
//! Both low-level and high-level usages are available.
//!
//! ## High-level usage through the `IdentifyTransport` struct
//!
//! This crate provides the `IdentifyTransport` struct, which wraps around a `Transport` and an
//! implementation of `Peerstore`. `IdentifyTransport` is itself a transport that accepts
//! multiaddresses of the form `/ipfs/...`.
//!
//! If you dial a multiaddr of the form `/ipfs/...`, then the `IdentifyTransport` will look into
//! the `Peerstore` for any known multiaddress for this peer and try to dial them using the
//! underlying transport. If you dial any other multiaddr, then it will dial this multiaddr using
//! the underlying transport, then negotiate the *identify* protocol with the remote in order to
//! obtain its ID, then add it to the peerstore, and finally dial the same multiaddr again and
//! return the connection.
//!
//! Listening doesn't support multiaddresses of the form `/ipfs/...`. Any address passed to
//! `listen_on` will be passed directly to the underlying transport.
//!
//! Whenever a remote connects to us, either through listening or through `next_incoming`, the
//! `IdentifyTransport` dials back the remote, upgrades the connection to the *identify* protocol
//! in order to obtain the ID of the remote, stores the information in the peerstore, and finally
//! only returns the connection. From the exterior, the multiaddress of the remote is of the form
//! `/ipfs/...`. If the remote doesn't support the *identify* protocol, then the socket is closed.
//!
//! Because of the behaviour of `IdentifyProtocol`, it is recommended to build it on top of a
//! `ConnectionReuse`.
//!
//! ## Low-level usage through the `IdentifyProtocolConfig` struct
//!
//! The `IdentifyProtocolConfig` struct implements the `ConnectionUpgrade` trait. Using it will
//! negotiate the *identify* protocol.
//!
//! The output of the upgrade is a `IdentifyOutput`. If we are the dialer, then `IdentifyOutput`
//! will contain the information sent by the remote. If we are the listener, then it will contain
//! a `IdentifySender` struct that can be used to transmit back to the remote the information about
//! it.

extern crate bytes;
extern crate futures;
extern crate multiaddr;
extern crate libp2p_peerstore;
extern crate libp2p_swarm;
extern crate protobuf;
extern crate tokio_io;
extern crate varint;

use bytes::{Bytes, BytesMut};
use futures::{future, Future, Stream, Sink, stream};
use libp2p_peerstore::{PeerId, Peerstore, PeerAccess};
use libp2p_swarm::{ConnectionUpgrade, Endpoint, Transport, MuxedTransport};
use multiaddr::{AddrComponent, Multiaddr};
use protobuf::Message as ProtobufMessage;
use protobuf::core::parse_from_bytes as protobuf_parse_from_bytes;
use protobuf::repeated::RepeatedField;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter;
use std::ops::Deref;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::Framed;
use varint::VarintCodec;

mod structs_proto;

/// Implementation of `Transport`. See the crate root description.
#[derive(Debug, Clone)]
pub struct IdentifyTransport<T, P> {
	transport: T,
	peerstore: P,
}

impl<T, P> IdentifyTransport<T, P> {
	/// Creates an `IdentifyTransport` that wraps around the given transport and peerstore.
	#[inline]
	pub fn new(transport: T, peerstore: P) -> IdentifyTransport<T, P> {
		IdentifyTransport {
			transport: transport,
			peerstore: peerstore,
		}
	}
}

impl<T, Pr, P> Transport for IdentifyTransport<T, Pr>
where
	T: Transport + Clone + 'static,			// TODO: 'static :(
	Pr: Deref<Target = P> + Clone + 'static,			// TODO: 'static :(
	for<'r> &'r P: Peerstore,
{
	type RawConn = T::RawConn;
	type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
	type ListenerUpgrade = Box<Future<Item = (T::RawConn, Multiaddr), Error = IoError>>;
	type Dial = Box<Future<Item = T::RawConn, Error = IoError>>;

	#[inline]
	fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
		// Note that `listen_on` expects a "regular" multiaddr (eg. `/ip/.../tcp/...`),
		// and not `/ipfs/<foo>`.

		let (listener, new_addr) = match self.transport.clone().listen_on(addr.clone()) {
			Ok((l, a)) => (l, a),
			Err((inner, addr)) => {
				let id = IdentifyTransport { transport: inner, peerstore: self.peerstore };
				return Err((id, addr));
			}
		};

		let identify_upgrade = self.transport
			.with_upgrade(IdentifyProtocolConfig);
		let peerstore = self.peerstore;

		let listener = listener
			.map(move |connec| {
				let identify_upgrade = identify_upgrade.clone();
				let fut = connec
					.and_then(move |(connec, client_addr)| {
						identify_upgrade
							.clone()
							.dial(client_addr.clone())
							.map_err(|_| {
								IoError::new(IoErrorKind::Other, "couldn't dial back incoming node")
							})
							.map(move |id| (id, connec))
					})
					.and_then(move |(dial, connec)| {
						dial.map(move |dial| (dial, connec))
					})
					.map(move |(identify, connec)| {
						let client_addr: Multiaddr = if let IdentifyOutput::RemoteInfo { info, .. } = identify {
							println!("received {:?}", info);
							//peerstore
							//	.peer_or_create(info.public_key)		// TODO: is this the public key?
							AddrComponent::IPFS(info.public_key).into()
						} else {
							unreachable!("the identify protocol guarantees that we receive remote \
											information when we dial a node")
						};

						(connec, client_addr)
					});
				Box::new(fut) as Box<Future<Item = _, Error = _>>
			});

		Ok((Box::new(listener) as Box<_>, new_addr))
	}

	#[inline]
	fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
		match multiaddr_to_peerid(addr) {
			Ok(peer_id) => {
				// If the multiaddress is a peer ID, try each known multiaddress (taken from the
				// peerstore) one by one.
				let addrs = self.peerstore
					.deref()
					.peer(&peer_id)
					.into_iter()
					.flat_map(|peer| {
						peer.addrs()
					})
					.collect::<Vec<_>>()
					.into_iter();

				let transport = self.transport;
				let future = stream::iter_ok(addrs)
					// Try to dial each address through the transport.
					.filter_map(move |addr| transport.clone().dial(addr).ok())
					.and_then(move |dial| dial)
					// Pick the first non-failing dial result.
					.then(|res| Ok(res))
					.filter_map(|res| res.ok())
					.into_future()
					.map_err(|(err, _)| err)
					.and_then(|(val, _)| val.ok_or(IoErrorKind::InvalidData.into()));		// TODO: wrong error

				Ok(Box::new(future) as Box<_>)
			},

			Err(addr) => {
				// If the multiaddress is something else, propagate it to the underlying transport
				// and identify the node.
				let transport = self.transport;
				let identify_upgrade = transport.clone()
					.with_upgrade(IdentifyProtocolConfig);

				let dial = match identify_upgrade.dial(addr.clone()) {
					Ok(d) => d,
					Err((_, addr)) => {
						let id = IdentifyTransport { transport, peerstore: self.peerstore };
						return Err((id, addr));
					}
				};

				let future = dial
					.and_then(|identify| {
						if let IdentifyOutput::RemoteInfo { info, .. } = identify {
							//peerstore
							//	.peer_or_create(info.public_key)		// TODO: is this the public key?
						} else {
							unreachable!("the identify protocol guarantees that we receive remote \
											information when we dial a node")
						}

						transport.dial(addr)
							.unwrap_or_else(|_| {
								panic!("the same multiaddr was determined to be valid earlier")
							})
					});
				
				Ok(Box::new(future) as Box<_>)
			}
		}

	}
}

// If the multiaddress is in the form `/ipfs/...`, turn it into a `PeerId`.
// Otherwise, return it as-is.
fn multiaddr_to_peerid(addr: Multiaddr) -> Result<PeerId, Multiaddr> {
	let components = addr.iter().collect::<Vec<_>>();
	if components.len() < 1 {
		return Err(addr);
	}

	match components.last() {
		Some(&AddrComponent::IPFS(ref peer_id)) => {	// TODO: `peer_id` is in fact a CID here
			match PeerId::from_bytes(peer_id.clone()) {
				Ok(peer_id) => Ok(peer_id),
				Err(_) => Err(addr)
			}
		},
		_ => Err(addr),
	}
}

impl<T, Pr, P> MuxedTransport for IdentifyTransport<T, Pr>
where
	T: MuxedTransport + Clone + 'static,
	Pr: Deref<Target = P> + Clone + 'static,
	for<'r> &'r P: Peerstore,
{
	type Incoming = Box<Future<Item = (T::RawConn, Multiaddr), Error = IoError>>;

	#[inline]
	fn next_incoming(self) -> Self::Incoming {
		let identify_upgrade = self.transport.clone().with_upgrade(IdentifyProtocolConfig);

		let future = self.transport
			.next_incoming()
			.and_then(move |(connec, client_addr)| {
				identify_upgrade
					.clone()
					.dial(client_addr.clone())
					.map_err(|_| {
						IoError::new(IoErrorKind::Other, "couldn't dial back incoming node")
					})
					.map(move |id| (id, connec))
			})
			.and_then(move |(dial, connec)| {
				dial.map(move |dial| (dial, connec))
			})
			.map(move |(identify, connec)| {
				let client_addr: Multiaddr = if let IdentifyOutput::RemoteInfo { info, .. } = identify {
					println!("received {:?}", info);
					//peerstore
					//	.peer_or_create(info.public_key)		// TODO: is this the public key?
					AddrComponent::IPFS(info.public_key).into()
				} else {
					unreachable!("the identify protocol guarantees that we receive remote \
									information when we dial a node")
				};

				(connec, client_addr)
			});

		Box::new(future) as Box<_>
	}
}

/// Configuration for an upgrade to the identity protocol.
#[derive(Debug, Clone)]
pub struct IdentifyProtocolConfig;

/// Output of the connection upgrade.
pub enum IdentifyOutput<T> {
	/// We obtained information from the remote. Happens when we are the dialer.
	RemoteInfo {
		info: IdentifyInfo,
		/// Address the remote sees for us.
		observed_addr: Multiaddr,
	},

	/// We opened a connection to the remote and need to send it information. Happens when we are
	/// the listener.
	Sender {
		/// Object used to send identify info to the client.
		sender: IdentifySender<T>,
		/// Observed multiaddress of the client.
		observed_addr: Multiaddr,
	},
}

/// Object used to send back information to the client.
pub struct IdentifySender<T> {
	future: Framed<T, VarintCodec<Vec<u8>>>,
}

impl<'a, T> IdentifySender<T> where T: AsyncWrite + 'a {
	/// Sends back information to the client. Returns a future that is signalled whenever the
	/// info have been sent.
	pub fn send(self, info: IdentifyInfo, observed_addr: &Multiaddr)
				-> Box<Future<Item = (), Error = IoError> + 'a>
	{
		let listen_addrs = info.listen_addrs
							   .into_iter()
							   .map(|addr| addr.to_string().into_bytes())
							   .collect();

		let mut message = structs_proto::Identify::new();
		message.set_agentVersion(info.agent_version);
		message.set_protocolVersion(info.protocol_version);
		message.set_publicKey(info.public_key);
		message.set_listenAddrs(listen_addrs);
		message.set_observedAddr(observed_addr.to_string().into_bytes());
		message.set_protocols(RepeatedField::from_vec(info.protocols));

		let bytes = message.write_to_bytes()
			.expect("writing protobuf failed ; should never happen");

		let future = self.future
			.send(bytes)
			.map(|_| ());
		Box::new(future) as Box<_>
	}
}

/// Information sent from the listener to the dialer.
#[derive(Debug, Clone)]
pub struct IdentifyInfo {
	/// Public key of the node.
	pub public_key: Vec<u8>,
	/// Version of the "global" protocol, eg. `ipfs/1.0.0` or `polkadot/1.0.0`.
	pub protocol_version: String,
	/// Name and version of the client. Can be thought as similar to the `User-Agent` header
	/// of HTTP.
	pub agent_version: String,
	/// Addresses that the remote is listening on.
	pub listen_addrs: Vec<Multiaddr>,
	/// Protocols supported by the remote.
	pub protocols: Vec<String>,
}

impl<C> ConnectionUpgrade<C> for IdentifyProtocolConfig
    where C: AsyncRead + AsyncWrite + 'static
{
	type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
	type UpgradeIdentifier = ();
	type Output = IdentifyOutput<C>;
	type Future = Box<Future<Item = Self::Output, Error = IoError>>;

	#[inline]
	fn protocol_names(&self) -> Self::NamesIter {
		iter::once((Bytes::from("/ipfs/id/1.0.0"), ()))
	}

	fn upgrade(self, socket: C, _: (), ty: Endpoint, observed_addr: &Multiaddr) -> Self::Future {
		let socket = socket.framed(VarintCodec::default());

		match ty {
			Endpoint::Dialer => {
				let future = socket.into_future()
				      .map(|(msg, _)| msg)
					  .map_err(|(err, _)| err)
					  .and_then(|msg| if let Some(msg) = msg {
					let (info, observed_addr) = parse_proto_msg(msg)?;
					Ok(IdentifyOutput::RemoteInfo { info, observed_addr })
				} else {
					Err(IoErrorKind::InvalidData.into())
				});

				Box::new(future) as Box<_>
			}

			Endpoint::Listener => {
				let sender = IdentifySender {
					future: socket,
				};

				let future = future::ok(IdentifyOutput::Sender {
					sender,
					observed_addr: observed_addr.clone(),
				});

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
			let listen_addrs = {
				let mut addrs = Vec::new();
				for addr in msg.take_listenAddrs().into_iter() {
					addrs.push(bytes_to_multiaddr(addr)?);
				}
				addrs
			};

			let observed_addr = bytes_to_multiaddr(msg.take_observedAddr())?;

			let info = IdentifyInfo {
				public_key: msg.take_publicKey(),
				protocol_version: msg.take_protocolVersion(),
				agent_version: msg.take_agentVersion(),
				listen_addrs: listen_addrs,
				protocols: msg.take_protocols().into_vec(),
			};

			Ok((info, observed_addr))
		}

		Err(err) => {
			Err(IoError::new(IoErrorKind::InvalidData, err))
		}
	}
}

// Turn a `Vec<u8>` into a `Multiaddr`. If something bad happens, turn it into an `IoError`.
fn bytes_to_multiaddr(bytes: Vec<u8>) -> Result<Multiaddr, IoError> {
	Multiaddr::from_bytes(bytes)
		.map_err(|err| {
			IoError::new(IoErrorKind::InvalidData, err)
		})
}

#[cfg(test)]
mod tests {
	extern crate libp2p_tcp_transport;
	extern crate tokio_core;

	use self::libp2p_tcp_transport::TcpConfig;
	use self::tokio_core::reactor::Core;
	use IdentifyTransport;
	use futures::{Future, Stream};
	use libp2p_peerstore::{PeerId, Peerstore, PeerAccess};
	use libp2p_peerstore::memory_peerstore::MemoryPeerstore;
	use libp2p_swarm::Transport;
	use multiaddr::{AddrComponent, Multiaddr};
	use std::io::Error as IoError;
	use std::iter;
	use std::time::Duration;
	use std::sync::Arc;

	#[test]
	fn dial_peer_id() {
		// When we dial an `/ipfs/...` address, the `IdentifyTransport` should look into the
		// peerstore and dial one of the known multiaddresses of the node instead.

		#[derive(Debug, Clone)]
		struct UnderlyingTrans { inner: TcpConfig }
		impl Transport for UnderlyingTrans {
			type RawConn = <TcpConfig as Transport>::RawConn;
			type Listener = Box<Stream<Item = Self::ListenerUpgrade, Error = IoError>>;
			type ListenerUpgrade = Box<Future<Item = (Self::RawConn, Multiaddr), Error = IoError>>;
			type Dial = <TcpConfig as Transport>::Dial;
			#[inline]
			fn listen_on(self, _: Multiaddr) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
				unreachable!()
			}
			#[inline]
			fn dial(self, addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
				assert_eq!(addr, "/ip4/127.0.0.1/tcp/12345".parse::<Multiaddr>().unwrap());
				Ok(self.inner.dial(addr).unwrap_or_else(|_| panic!()))
			}
		}

		let peer_id = PeerId::from_public_key(&vec![1, 2, 3, 4]);

		let peerstore = MemoryPeerstore::empty();
		peerstore
			.peer_or_create(&peer_id)
			.add_addr("/ip4/127.0.0.1/tcp/12345".parse().unwrap(), Duration::from_secs(3600));

		let mut core = Core::new().unwrap();
		let underlying = UnderlyingTrans { inner: TcpConfig::new(core.handle()) };
		let transport = IdentifyTransport::new(underlying, Arc::new(peerstore));

		let future = transport
			.dial(iter::once(AddrComponent::IPFS(peer_id.into_bytes())).collect())
			.unwrap_or_else(|_| panic!())
			.then::<_, Result<(), ()>>(|_| Ok(()));

		let _ = core.run(future).unwrap();
	}
}
