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
use libp2p_core::{Multiaddr, upgrade::{InboundUpgrade, OutboundUpgrade, UpgradeInfo}};
use std::{io, iter};
use tokio_codec::{FramedRead, FramedWrite};
use tokio_io::{AsyncRead, AsyncWrite};
use unsigned_varint::codec::UviBytes;

/// The connection upgrade type to retrieve or report externally visible addresses.
pub struct Observed {}

impl Observed {
    pub fn new() -> Self {
        Observed {}
    }
}

impl UpgradeInfo for Observed {
    type UpgradeId = ();
    type NamesIter = iter::Once<(Bytes, Self::UpgradeId)>;

    fn protocol_names(&self) -> Self::NamesIter {
        iter::once((Bytes::from("/paritytech/observed-address/0.1.0"), ()))
    }
}

impl<C> InboundUpgrade<C> for Observed
where
    C: AsyncRead + AsyncWrite + Send + 'static
{
    type Output = Sender<C>;
    type Error = io::Error;
    type Future = Box<dyn Future<Item=Self::Output, Error=Self::Error> + Send>;

    fn upgrade_inbound(self, conn: C, _: ()) -> Self::Future {
        let io = FramedWrite::new(conn, UviBytes::default());
        Box::new(future::ok(Sender { io }))
    }
}

impl<C> OutboundUpgrade<C> for Observed
where
    C: AsyncRead + AsyncWrite + Send + 'static
{
    type Output = Multiaddr;
    type Error = io::Error;
    type Future = Box<dyn Future<Item=Self::Output, Error=Self::Error> + Send>;

    fn upgrade_outbound(self, conn: C, _: ()) -> Self::Future {
        let io = FramedRead::new(conn, UviBytes::default());
        let future = io.into_future()
            .map_err(|(e, _): (io::Error, FramedRead<C, UviBytes>)| e)
            .and_then(move |(bytes, _)| {
                if let Some(b) = bytes {
                    let ma = Multiaddr::from_bytes(b.to_vec())
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                    Ok(ma)
                } else {
                    Err(io::ErrorKind::InvalidData.into())
                }
            });
        Box::new(future)
    }
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

#[cfg(test)]
mod tests {
    extern crate tokio;

    use libp2p_core::{Multiaddr, upgrade::{InboundUpgrade, OutboundUpgrade}};
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
                Observed::new().upgrade_inbound(conn.unwrap(), ())
            })
            .and_then(move |sender| sender.send_address(observed_addr1));

        let client = TcpStream::connect(&server_addr)
            .map_err(|e| e.into())
            .and_then(|conn| {
                Observed::new().upgrade_outbound(conn, ())
            })
            .map(move |addr| {
                eprintln!("{} {}", addr, observed_addr2);
                assert_eq!(addr, observed_addr2)
            });

        current_thread::block_on_all(future::lazy(move || {
            current_thread::spawn(server.map_err(|e| panic!("server error: {}", e)).map(|_| ()));
            client.map_err(|e| panic!("client error: {}", e))
        }))
        .unwrap();
    }
}
