// Copyright 2017-2018 Parity Technologies (UK) Ltd.
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

//! QUIC transport for *libp2p*
use quinn_proto::{Connection, Transmit};
use std::{net::{UdpSocket, addr::SocketAddr}, io::ErrorKind, time::Instant};
use futures::prelude::*;

struct QuicTransport {
	connection: Connection,
	socket: UdpSocket,
	transmit: Option<Transmit>,
	offset: usize,
}

impl QuicTransport {
	fn poll_transmit(&mut self) -> Result<(), std::io::Error> {
		self.transmit = match self.transmit.or_else(|| self.connection.poll_transmit(Instant::now())) {
			Some(s) => s,
			None => return Ok(()),
		};
		while self.transmit.contents.len() > self.offest {
			match self.socket.send_to(contents[self.offset..], destination) {
				Ok(len) => self.offset += len,
				Err(e) => match e.kind() {
					ErrorKind::Interrupted => continue,
					ErrorKind::WouldBlock => unimplemented!("figure out what to wake!"),
					_ => return Err(e),
				},
			}
		}
		Ok(())
	}
}
