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

extern crate bytes;
extern crate futures;
extern crate libp2p_secio as secio;
extern crate libp2p_swarm as swarm;
extern crate libp2p_tcp_transport as tcp;
extern crate tokio_core;
extern crate tokio_io;

use bytes::BytesMut;
use futures::{Stream, Sink, Future};
use swarm::{Transport, SimpleProtocol};
use tcp::TcpConfig;
use tokio_core::reactor::Core;
use tokio_io::codec::length_delimited;

fn main() {
    let mut core = Core::new().unwrap();
    let tcp = TcpConfig::new(core.handle());
    
    let with_secio = tcp
        .with_upgrade(swarm::PlainTextConfig)
        .or_upgrade({
            let private_key = include_bytes!("test-private-key.pk8");
            let public_key = include_bytes!("test-public-key.der").to_vec();
            secio::SecioConfig {
                key: secio::SecioKeyPair::rsa_from_pkcs8(private_key, public_key).unwrap(),
            }
        });

    let with_echo = with_secio.with_upgrade(SimpleProtocol::new("/echo/1.0.0", |socket| {
        Ok(length_delimited::Framed::<_, BytesMut>::new(socket))
    }));

    let dialer = with_echo.dial(swarm::multiaddr::Multiaddr::new("/ip4/127.0.0.1/tcp/10333").unwrap())
        .unwrap_or_else(|_| panic!())
        .and_then(|f| {
            f.send("hello world".into())
        })
        .and_then(|f| {
            f.into_future()
                .map(|(msg, rest)| {
                    println!("received: {:?}", msg);
                    rest
                })
                .map_err(|(err, _)| err)
        });

    core.run(dialer).unwrap();
}
