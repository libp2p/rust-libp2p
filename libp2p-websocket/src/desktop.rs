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

use futures::{stream, Future, IntoFuture, Sink, Stream};
use multiaddr::{AddrComponent, Multiaddr};
use rw_stream_sink::RwStreamSink;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use swarm::Transport;
use websocket::client::builder::ClientBuilder;
use websocket::message::OwnedMessage;
use websocket::server::upgrade::async::IntoWs;
use websocket::stream::async::Stream as AsyncStream;

/// Represents the configuration for a websocket transport capability for libp2p. Must be put on
/// top of another `Transport`.
///
/// This implementation of `Transport` accepts any address that ends with `/ws` or `/wss`, and will
/// try to pass the underlying multiaddress to the underlying `Transport`.
///
/// > **Note**: `/wss` is only supported for dialing and not listening.
#[derive(Debug, Clone)]
pub struct WsConfig<T> {
	transport: T,
}

impl<T> WsConfig<T> {
	/// Creates a new configuration object for websocket.
	///
	/// The websockets will run on top of the `Transport` you pass as parameter.
	#[inline]
	pub fn new(inner: T) -> WsConfig<T> {
		WsConfig { transport: inner }
	}
}

impl<T> Transport for WsConfig<T>
where
	T: Transport + 'static, // TODO: this 'static is pretty arbitrary and is necessary because of the websocket library
	T::RawConn: Send, // TODO: this Send is pretty arbitrary and is necessary because of the websocket library
{
	type RawConn = Box<AsyncStream>;
	type Listener = stream::Map<
		T::Listener,
		fn((<T as Transport>::ListenerUpgrade, Multiaddr)) -> (Self::ListenerUpgrade, Multiaddr),
	>;
	type ListenerUpgrade = Box<Future<Item = Self::RawConn, Error = IoError>>;
	type Dial = Box<Future<Item = Self::RawConn, Error = IoError>>;

	fn listen_on(
		self,
		original_addr: Multiaddr,
	) -> Result<(Self::Listener, Multiaddr), (Self, Multiaddr)> {
		let mut inner_addr = original_addr.clone();
		match inner_addr.pop() {
			Some(AddrComponent::WS) => {}
			_ => return Err((self, original_addr)),
		};

		let (inner_listen, new_addr) = match self.transport.listen_on(inner_addr) {
			Ok((listen, mut new_addr)) => {
				// Need to suffix `/ws` to the listening address.
				new_addr.append(AddrComponent::WS);
				(listen, new_addr)
			}
			Err((transport, _)) => {
				return Err((
					WsConfig {
						transport: transport,
					},
					original_addr,
				));
			}
		};

		let listen = inner_listen.map::<_, fn(_) -> _>(|(stream, mut client_addr)| {
			// Need to suffix `/ws` to each client address.
			client_addr.append(AddrComponent::WS);

			// Upgrade the listener to websockets like the websockets library requires us to do.
			let upgraded = stream.and_then(|stream| {
				stream
					.into_ws()
					.map_err(|e| IoError::new(IoErrorKind::Other, e.3))
					.and_then(|stream| {
						// Accept the next incoming connection.
						stream
							.accept()
							.map_err(|err| IoError::new(IoErrorKind::Other, err))
							.map(|(client, _http_headers)| {
								// Plug our own API on top of the `websockets` API.
								let framed_data = client
									.map_err(|err| IoError::new(IoErrorKind::Other, err))
									.sink_map_err(|err| IoError::new(IoErrorKind::Other, err))
									.with(|data| Ok(OwnedMessage::Binary(data)))
									.and_then(|recv| {
										match recv {
											OwnedMessage::Binary(data) => Ok(Some(data)),
											OwnedMessage::Text(data) => Ok(Some(data.into_bytes())),
											OwnedMessage::Close(_) => Ok(None),
											// TODO: handle pings and pongs, which is freaking hard
											//		 for now we close the socket when that happens
											_ => Ok(None)
										}
									})
									// TODO: is there a way to merge both lines into one?
									.take_while(|v| Ok(v.is_some()))
									.map(|v| v.expect("we only take while this is Some"));

								let read_write = RwStreamSink::new(framed_data);
								Box::new(read_write) as Box<AsyncStream>
							})
					})
					.map(|s| Box::new(Ok(s).into_future()) as Box<Future<Item = _, Error = _>>)
					.into_future()
					.flatten()
			});

			(
				Box::new(upgraded) as Box<Future<Item = _, Error = _>>,
				client_addr,
			)
		});

		Ok((listen, new_addr))
	}

	fn dial(self, original_addr: Multiaddr) -> Result<Self::Dial, (Self, Multiaddr)> {
		let mut inner_addr = original_addr.clone();
		let is_wss = match inner_addr.pop() {
			Some(AddrComponent::WS) => false,
			Some(AddrComponent::WSS) => true,
			_ => return Err((self, original_addr)),
		};

		let inner_dial = match self.transport.dial(inner_addr) {
			Ok(d) => d,
			Err((transport, _)) => {
				return Err((
					WsConfig {
						transport: transport,
					},
					original_addr,
				));
			}
		};

		let dial = inner_dial.into_future().and_then(move |connec| {
			// We pass a dummy address to `ClientBuilder` because it is never used anywhere
			// in the negotiation anyway, and we use `async_connect_on` to pass a stream.
			ClientBuilder::new(if is_wss { "wss://127.0.0.1" } else { "ws://127.0.0.1" })
				.expect("hard-coded ws address is always valid")
				.async_connect_on(connec)
				.map_err(|err| IoError::new(IoErrorKind::Other, err))
				.map(|(client, _)| {
					// Plug our own API on top of the API of the websockets library.
					let framed_data = client
						.map_err(|err| IoError::new(IoErrorKind::Other, err))
						.sink_map_err(|err| IoError::new(IoErrorKind::Other, err))
						.with(|data| Ok(OwnedMessage::Binary(data)))
						.and_then(|recv| {
							match recv {
								OwnedMessage::Binary(data) => Ok(data),
								OwnedMessage::Text(data) => Ok(data.into_bytes()),
								// TODO: pings and pongs and close messages need to be
								//       answered ; and this is really hard ; for now we produce
								//		 an error when that happens
								_ => Err(IoError::new(IoErrorKind::Other, "unimplemented")),
							}
						});
					let read_write = RwStreamSink::new(framed_data);
					Box::new(read_write) as Box<AsyncStream>
				})
		});

		Ok(Box::new(dial) as Box<_>)
	}

	fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
		let mut server = server.clone();
		let last_proto = match server.pop() {
			Some(v @ AddrComponent::WS) | Some(v @ AddrComponent::WSS) => v,
			_ => return None,
		};

		let mut observed = observed.clone();
		match observed.pop() {
			Some(AddrComponent::WS) => false,
			Some(AddrComponent::WSS) => true,
			_ => return None,
		};

		self.transport.nat_traversal(&server, &observed)
			.map(move |mut result| { result.append(last_proto); result })
	}
}

#[cfg(test)]
mod tests {
	extern crate libp2p_tcp_transport as tcp;
	extern crate tokio_core;
	use self::tokio_core::reactor::Core;
	use WsConfig;
	use futures::{Future, Stream};
	use multiaddr::Multiaddr;
	use swarm::Transport;

	#[test]
	fn dialer_connects_to_listener_ipv4() {
		let mut core = Core::new().unwrap();
		let ws_config = WsConfig::new(tcp::TcpConfig::new(core.handle()));

		let (listener, addr) = ws_config
			.clone()
			.listen_on("/ip4/0.0.0.0/tcp/0/ws".parse().unwrap())
			.unwrap();
		assert!(addr.to_string().ends_with("/ws"));
		assert!(!addr.to_string().ends_with("/0/ws"));
		let listener = listener
			.into_future()
			.map_err(|(e, _)| e)
			.and_then(|(c, _)| c.unwrap().0);
		let dialer = ws_config.clone().dial(addr).unwrap();

		let future = listener
			.select(dialer)
			.map_err(|(e, _)| e)
			.and_then(|(_, n)| n);
		core.run(future).unwrap();
	}

	#[test]
	fn dialer_connects_to_listener_ipv6() {
		let mut core = Core::new().unwrap();
		let ws_config = WsConfig::new(tcp::TcpConfig::new(core.handle()));

		let (listener, addr) = ws_config
			.clone()
			.listen_on("/ip6/::1/tcp/0/ws".parse().unwrap())
			.unwrap();
		assert!(addr.to_string().ends_with("/ws"));
		assert!(!addr.to_string().ends_with("/0/ws"));
		let listener = listener
			.into_future()
			.map_err(|(e, _)| e)
			.and_then(|(c, _)| c.unwrap().0);
		let dialer = ws_config.clone().dial(addr).unwrap();

		let future = listener
			.select(dialer)
			.map_err(|(e, _)| e)
			.and_then(|(_, n)| n);
		core.run(future).unwrap();
	}

	#[test]
	fn nat_traversal() {
		let core = Core::new().unwrap();
		let ws_config = WsConfig::new(tcp::TcpConfig::new(core.handle()));

		{
			let server = "/ip4/127.0.0.1/tcp/10000/ws".parse::<Multiaddr>().unwrap();
			let observed = "/ip4/80.81.82.83/tcp/25000/ws".parse::<Multiaddr>().unwrap();
			assert_eq!(ws_config.nat_traversal(&server, &observed).unwrap(),
					"/ip4/80.81.82.83/tcp/10000/ws".parse::<Multiaddr>().unwrap());
		}

		{
			let server = "/ip4/127.0.0.1/tcp/10000/wss".parse::<Multiaddr>().unwrap();
			let observed = "/ip4/80.81.82.83/tcp/25000/wss".parse::<Multiaddr>().unwrap();
			assert_eq!(ws_config.nat_traversal(&server, &observed).unwrap(),
					"/ip4/80.81.82.83/tcp/10000/wss".parse::<Multiaddr>().unwrap());
		}

		{
			let server = "/ip4/127.0.0.1/tcp/10000/ws".parse::<Multiaddr>().unwrap();
			let observed = "/ip4/80.81.82.83/tcp/25000/wss".parse::<Multiaddr>().unwrap();
			assert_eq!(ws_config.nat_traversal(&server, &observed).unwrap(),
					"/ip4/80.81.82.83/tcp/10000/ws".parse::<Multiaddr>().unwrap());
		}

		{
			let server = "/ip4/127.0.0.1/tcp/10000/wss".parse::<Multiaddr>().unwrap();
			let observed = "/ip4/80.81.82.83/tcp/25000/ws".parse::<Multiaddr>().unwrap();
			assert_eq!(ws_config.nat_traversal(&server, &observed).unwrap(),
					"/ip4/80.81.82.83/tcp/10000/wss".parse::<Multiaddr>().unwrap());
		}
	}
}
