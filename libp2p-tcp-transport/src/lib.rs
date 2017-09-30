extern crate libp2p_transport as transport;
extern crate tokio_core;
extern crate tokio_io;
extern crate multiaddr;
extern crate futures;

use std::io::Error as IoError;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio_core::reactor::Core;
use tokio_core::net::{TcpStream, TcpListener, TcpStreamNew};
use futures::Future;
use futures::stream::Stream;
use multiaddr::{Multiaddr, Protocol};
use transport::Transport;

pub struct Tcp {
	pub event_loop: Core,
}

impl Tcp {
	pub fn new() -> Result<Tcp, IoError> {
		Ok(Tcp {
			event_loop: Core::new()?,
		})
	}
}

impl Transport for Tcp {
	/// The raw connection.
	type RawConn = TcpStream;

	/// The listener produces incoming connections.
	type Listener = Box<Stream<Item=Self::RawConn, Error=IoError>>;

	/// A future which indicates currently dialing to a peer.
	type Dial = TcpStreamNew;

	/// Listen on the given multi-addr.
	/// Returns the address back if it isn't supported.
	fn listen_on(&mut self, addr: Multiaddr) -> Result<Self::Listener, Multiaddr> {
		if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
			Ok(Box::new(futures::future::result(TcpListener::bind(&socket_addr, &self.event_loop.handle())).map(|listener| {
				// Pull out a stream of sockets for incoming connections
				listener.incoming().map(|x| x.0)
			}).flatten_stream()))
		} else {
			Err(addr)
		}
	}

	/// Dial to the given multi-addr.
	/// Returns either a future which may resolve to a connection,
	/// or gives back the multiaddress.
	fn dial(&mut self, addr: Multiaddr) -> Result<Self::Dial, Multiaddr> {
		if let Ok(socket_addr) = multiaddr_to_socketaddr(&addr) {
			Ok(TcpStream::connect(&socket_addr, &self.event_loop.handle()))
		} else {
			Err(addr)
		}
	}
}

// This type of logic should probably be moved into the multiaddr package
fn multiaddr_to_socketaddr(addr: &Multiaddr) -> Result<SocketAddr, &Multiaddr>  {
	let protocols = addr.protocol();
	match (protocols[0], protocols[1]) {
		(Protocol::IP4, Protocol::TCP) => {
			let bs = addr.as_slice();
			Ok(SocketAddr::new(
				IpAddr::V4(Ipv4Addr::new(bs[1], bs[2], bs[3], bs[4])),
				(bs[6] as u16) << 8 | bs[7] as u16
			))
		},
		(Protocol::IP6, Protocol::TCP) => {
			let bs = addr.as_slice();
			if let Ok(Some(s)) = Protocol::IP6.bytes_to_string(&bs[1..17]) {
				if let Ok(ipv6addr) = s.parse() {
					return Ok(SocketAddr::new(IpAddr::V6(ipv6addr), (bs[18] as u16) << 8 | bs[19] as u16))
				}
			}
			Err(addr)
		},
		_ => Err(addr),
	}
}

#[cfg(test)]
mod tests {
	use super::{Tcp, multiaddr_to_socketaddr};
	use std;
	use std::net::{IpAddr, Ipv4Addr, SocketAddr};
	use tokio_io;
	use futures::Future;
	use futures::stream::Stream;
	use multiaddr::Multiaddr;
	use transport::Transport;

	#[test]
	fn multiaddr_to_tcp_conversion() {
		use std::net::{Ipv6Addr};

		assert_eq!(
			multiaddr_to_socketaddr(&Multiaddr::new("/ip4/127.0.0.1/tcp/12345").unwrap()),
			Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345))
		);
		assert_eq!(
			multiaddr_to_socketaddr(&Multiaddr::new("/ip4/255.255.255.255/tcp/8080").unwrap()),
			Ok(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)), 8080))
		);
		assert_eq!(
			multiaddr_to_socketaddr(&Multiaddr::new("/ip6/::1/tcp/12345").unwrap()),
			Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), 12345))
		);
		assert_eq!(
			multiaddr_to_socketaddr(&Multiaddr::new("/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/tcp/8080").unwrap()),
			Ok(SocketAddr::new(IpAddr::V6(Ipv6Addr::new(65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535)), 8080))
		);
	}

	#[test]
	fn communicating_between_dialer_and_listener() {
		use std::io::Write;

		/// This thread is running the listener
		/// while the main thread runs the dialer
		std::thread::spawn(move || {
			let addr = Multiaddr::new("/ip4/127.0.0.1/tcp/12345").unwrap();
			let mut tcp = Tcp::new().unwrap();
			let handle = tcp.event_loop.handle();
			let listener = tcp.listen_on(addr).unwrap().for_each(|sock| {
				// Define what to do with the socket that just connected to us
				// Which in this case is read 3 bytes
				let handle_conn = tokio_io::io::read_exact(sock, [0; 3]).map(|(_, buf)| {
					assert_eq!(buf, [1,2,3])
				}).map_err(|err| {
					panic!("IO error {:?}", err)
				});

				// Spawn the future as a concurrent task
				handle.spawn(handle_conn);

				Ok(())
			});

			tcp.event_loop.run(listener).unwrap();
		});
		std::thread::sleep(std::time::Duration::from_millis(100));
		let addr = Multiaddr::new("/ip4/127.0.0.1/tcp/12345").unwrap();
		let mut tcp = Tcp::new().unwrap();
		// Obtain a future socket through dialing
		let socket = tcp.dial(addr.clone()).unwrap();
		// Define what to do with the socket once it's obtained
		let action = socket.then(|sock| {
			match sock {
				Ok(mut s) => {
					let written = s.write(&[0x1,0x2,0x3]).unwrap();
					Ok(written)
				}
				Err(x) => Err(x)
			}
		});
		// Execute the future in our event loop
		tcp.event_loop.run(action).unwrap();
		std::thread::sleep(std::time::Duration::from_millis(100));
	}
}
