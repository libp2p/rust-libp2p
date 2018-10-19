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

//! Connection upgrade to allow retrieving the externally visible address (as dialer) or
//! to report the externally visible address (as listener).

extern crate bytes;
extern crate futures;
extern crate libp2p_core;
extern crate tokio_codec;
extern crate tokio_io;
extern crate unsigned_varint;

use bytes::Bytes;
use futures::{future, prelude::*};
use libp2p_core::{ConnectionUpgrade, Endpoint, Multiaddr};
use std::{io, iter};
use tokio_codec::{FramedRead, FramedWrite};
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec::UviBytes;

/// The output, this connection upgrade produces.
pub enum Output<C> {
    /// As `Dialer`, we get our own externally observed address.
    Address(Multiaddr),
    /// As `Listener`, we return a sender which allows reporting the observed
    /// address the client.
    Sender(Sender<C>)
}

/// `Sender` allows reporting back the observed address to the remote endpoint.
pub struct Sender<C> {
    io: FramedWrite<C, UviBytes>
}

impl<C: AsyncWrite> Sender<C> {
    /// Send address `a` to remote as the observed address.
    pub fn send_address(self, a: Multiaddr) -> impl Future<Item=(), Error=io::Error> {
        self.io.send(Bytes::from(a.into_bytes())).map(|_io| ())
    }
}

/// The connection upgrade type to retrieve or report externally visible addresses.
pub struct Observed {}

impl Observed {
    pub fn new() -> Self {
        Observed {}
    }
}

impl<C> ConnectionUpgrade<C> for Observed
where
    C: AsyncRead + AsyncWrite + Send + 'static
{
    type NamesIter = iter::Once<(Bytes, Self::UpgradeIdentifier)>;
    type UpgradeIdentifier = ();
    type Output = Output<C>;
    type Future = Box<dyn Future<Item=Self::Output, Error=io::Error> + Send>;

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/paritytech/observed-address/0.1.0"), ()))
    }

    fn upgrade(self, conn: C, _: (), role: Endpoint) -> Self::Future {
        match role {
            Endpoint::Dialer => {
                let io = FramedRead::new(conn, UviBytes::default());
                let future = io.into_future()
                    .map_err(|(e, _): (io::Error, FramedRead<C, UviBytes>)| e)
                    .and_then(move |(bytes, _)| {
                        if let Some(b) = bytes {
                            let ma = Multiaddr::from_bytes(b.to_vec())
                                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                            Ok(Output::Address(ma))
                        } else {
                            Err(io::ErrorKind::InvalidData.into())
                        }
                    });
                Box::new(future)
            }
            Endpoint::Listener => {
                let io = FramedWrite::new(conn, UviBytes::default());
                Box::new(future::ok(Output::Sender(Sender { io })))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate tokio;

    use libp2p_core::{ConnectionUpgrade, Endpoint, Multiaddr};
    use self::tokio::runtime::current_thread;
    use self::tokio::net::{TcpListener, TcpStream};
    use super::*;

    #[test]
    fn observed_address() {
        let server = TcpListener::bind(&"127.0.0.1:0".parse().unwrap()).unwrap();
        let server_addr = server.local_addr().unwrap();

        let observed_addr1: Multiaddr = "/ip4/127.0.0.1/tcp/10000".parse().unwrap();
        let observed_addr2 = observed_addr1.clone();

        let server = server.incoming()
            .into_future()
            .map_err(|(e, _)| e.into())
            .and_then(move |(conn, _)| {
                Observed::new().upgrade(conn.unwrap(), (), Endpoint::Listener)
            })
            .and_then(move |output| {
                match output {
                    Output::Sender(s) => s.send_address(observed_addr1),
                    Output::Address(_) => unreachable!()
                }
            });

        let client = TcpStream::connect(&server_addr)
            .map_err(|e| e.into())
            .and_then(|conn| {
                Observed::new().upgrade(conn, (), Endpoint::Dialer)
            })
            .map(move |output| {
                match output {
                    Output::Address(addr) => {
                        eprintln!("{} {}", addr, observed_addr2);
                        assert_eq!(addr, observed_addr2)
                    }
                    _ => unreachable!()
                }
            });

        current_thread::block_on_all(future::lazy(move || {
            current_thread::spawn(server.map_err(|e| panic!("server error: {}", e)).map(|_| ()));
            client.map_err(|e| panic!("client error: {}", e))
        }))
        .unwrap();
    }
}
