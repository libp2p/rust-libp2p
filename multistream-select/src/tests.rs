#![cfg(test)]

use {listener_select_proto, dialer_select_proto};
use ProtocolChoiceError;
use bytes::Bytes;
use dialer_select::{dialer_select_proto_parallel, dialer_select_proto_serial};
use futures::{Sink, Stream};
use futures::Future;
use protocol::{Dialer, Listener, DialerToListenerMessage, ListenerToDialerMessage};
use tokio_core::net::TcpListener;
use tokio_core::net::TcpStream;
use tokio_core::reactor::Core;

#[test]
fn negociate_with_self_succeeds() {
	let mut core = Core::new().unwrap();

	let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap();
	let listener_addr = listener.local_addr().unwrap();

	let server = listener.incoming()
	                     .into_future()
	                     .map_err(|(e, _)| e.into())
	                     .and_then(move |(connec, _)| Listener::new(connec.unwrap().0))
	                     .and_then(|l| l.into_future().map_err(|(e, _)| e))
	                     .and_then(|(msg, rest)| {
		let proto = match msg {
			Some(DialerToListenerMessage::ProtocolRequest { name }) => name,
			_ => panic!(),
		};
		rest.send(ListenerToDialerMessage::ProtocolAck { name: proto })
	});

	let client = TcpStream::connect(&listener_addr, &core.handle())
		.from_err()
		.and_then(move |stream| Dialer::new(stream))
		.and_then(move |dialer| {
			let p = Bytes::from("/hello/1.0.0");
			dialer.send(DialerToListenerMessage::ProtocolRequest { name: p })
		})
		.and_then(move |dialer| dialer.into_future().map_err(|(e, _)| e))
		.and_then(move |(msg, _)| {
			let proto = match msg {
				Some(ListenerToDialerMessage::ProtocolAck { name }) => name,
				_ => panic!(),
			};
			assert_eq!(proto, "/hello/1.0.0");
			Ok(())
		});

	core.run(server.join(client)).unwrap();
}

#[test]
fn select_proto_basic() {
	let mut core = Core::new().unwrap();

	let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap();
	let listener_addr = listener.local_addr().unwrap();

	let server = listener.incoming()
	                     .into_future()
	                     .map(|s| s.0.unwrap().0)
	                     .map_err(|(e, _)| e.into())
	                     .and_then(move |connec| {
		let protos = vec![
			(Bytes::from("/proto1"), <Bytes as PartialEq>::eq, 0),
			(Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 1),
		]
		             .into_iter();
		listener_select_proto(connec, protos).map(|r| r.0)
	});

	let client =
		TcpStream::connect(&listener_addr, &core.handle()).from_err().and_then(move |connec| {
			let protos = vec![
				(Bytes::from("/proto3"), <Bytes as PartialEq>::eq, 2),
				(Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 3),
			]
			             .into_iter();
			dialer_select_proto(connec, protos).map(|r| r.0)
		});

	let (dialer_chosen, listener_chosen) = core.run(client.join(server)).unwrap();
	assert_eq!(dialer_chosen, 3);
	assert_eq!(listener_chosen, 1);
}

#[test]
fn no_protocol_found() {
	let mut core = Core::new().unwrap();

	let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap();
	let listener_addr = listener.local_addr().unwrap();

	let server = listener.incoming()
	                     .into_future()
	                     .map(|s| s.0.unwrap().0)
	                     .map_err(|(e, _)| e.into())
	                     .and_then(move |connec| {
		let protos = vec![
			(Bytes::from("/proto1"), <Bytes as PartialEq>::eq, 1),
			(Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 2),
		]
		             .into_iter();
		listener_select_proto(connec, protos).map(|r| r.0)
	});

	let client =
		TcpStream::connect(&listener_addr, &core.handle()).from_err().and_then(move |connec| {
			let protos = vec![
				(Bytes::from("/proto3"), <Bytes as PartialEq>::eq, 3),
				(Bytes::from("/proto4"), <Bytes as PartialEq>::eq, 4),
			]
			             .into_iter();
			dialer_select_proto(connec, protos).map(|r| r.0)
		});

	match core.run(client.join(server)) {
		Err(ProtocolChoiceError::NoProtocolFound) => (),
		_ => panic!(),
	}
}

#[test]
#[ignore] // TODO: not yet implemented in the listener
fn select_proto_parallel() {
	let mut core = Core::new().unwrap();

	let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap();
	let listener_addr = listener.local_addr().unwrap();

	let server = listener.incoming()
	                     .into_future()
	                     .map(|s| s.0.unwrap().0)
	                     .map_err(|(e, _)| e.into())
	                     .and_then(move |connec| {
		let protos = vec![
			(Bytes::from("/proto1"), <Bytes as PartialEq>::eq, 0),
			(Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 1),
		]
		             .into_iter();
		listener_select_proto(connec, protos).map(|r| r.0)
	});

	let client =
		TcpStream::connect(&listener_addr, &core.handle()).from_err().and_then(move |connec| {
			let protos = vec![
				(Bytes::from("/proto3"), <Bytes as PartialEq>::eq, 2),
				(Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 3),
			]
			             .into_iter();
			dialer_select_proto_parallel(connec, protos).map(|r| r.0)
		});

	let (dialer_chosen, listener_chosen) = core.run(client.join(server)).unwrap();
	assert_eq!(dialer_chosen, 3);
	assert_eq!(listener_chosen, 1);
}

#[test]
fn select_proto_serial() {
	let mut core = Core::new().unwrap();

	let listener = TcpListener::bind(&"127.0.0.1:0".parse().unwrap(), &core.handle()).unwrap();
	let listener_addr = listener.local_addr().unwrap();

	let server = listener.incoming()
	                     .into_future()
	                     .map(|s| s.0.unwrap().0)
	                     .map_err(|(e, _)| e.into())
	                     .and_then(move |connec| {
		let protos = vec![
			(Bytes::from("/proto1"), <Bytes as PartialEq>::eq, 0),
			(Bytes::from("/proto2"), <Bytes as PartialEq>::eq, 1),
		]
		             .into_iter();
		listener_select_proto(connec, protos).map(|r| r.0)
	});

	let client =
		TcpStream::connect(&listener_addr, &core.handle()).from_err().and_then(move |connec| {
			let protos = vec![(Bytes::from("/proto3"), 2), (Bytes::from("/proto2"), 3)].into_iter();
			dialer_select_proto_serial(connec, protos).map(|r| r.0)
		});

	let (dialer_chosen, listener_chosen) = core.run(client.join(server)).unwrap();
	assert_eq!(dialer_chosen, 3);
	assert_eq!(listener_chosen, 1);
}
