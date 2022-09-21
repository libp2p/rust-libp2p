// Copyright 2019 Parity Technologies (UK) Ltd.
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

use futures::prelude::*;
use libp2p::core::identity;
use libp2p::core::transport::{MemoryTransport, Transport};
use libp2p::core::upgrade::{self, InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p::mplex::MplexConfig;
use libp2p::noise;
use multiaddr::{Multiaddr, Protocol};
use rand::random;
use std::{io, pin::Pin};

#[derive(Clone)]
struct HelloUpgrade {}

impl UpgradeInfo for HelloUpgrade {
    type Info = &'static str;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once("/hello/1")
    }
}

impl<C> InboundUpgrade<C> for HelloUpgrade
where
    C: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    type Output = C;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, mut socket: C, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let mut buf = [0u8; 5];
            socket.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf[..], "hello".as_bytes());
            Ok(socket)
        })
    }
}

impl<C> OutboundUpgrade<C> for HelloUpgrade
where
    C: AsyncWrite + AsyncRead + Send + Unpin + 'static,
{
    type Output = C;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, mut socket: C, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            socket.write_all(b"hello").await.unwrap();
            socket.flush().await.unwrap();
            Ok(socket)
        })
    }
}

#[test]
fn upgrade_pipeline() {
    let listener_keys = identity::Keypair::generate_ed25519();
    let listener_id = listener_keys.public().to_peer_id();
    let mut listener_transport = MemoryTransport::default()
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::NoiseAuthenticated::xx(&listener_keys).unwrap())
        .apply(HelloUpgrade {})
        .apply(HelloUpgrade {})
        .apply(HelloUpgrade {})
        .multiplex(MplexConfig::default())
        .boxed();

    let dialer_keys = identity::Keypair::generate_ed25519();
    let dialer_id = dialer_keys.public().to_peer_id();
    let mut dialer_transport = MemoryTransport::default()
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::NoiseAuthenticated::xx(&dialer_keys).unwrap())
        .apply(HelloUpgrade {})
        .apply(HelloUpgrade {})
        .apply(HelloUpgrade {})
        .multiplex(MplexConfig::default())
        .boxed();

    let listen_addr1 = Multiaddr::from(Protocol::Memory(random::<u64>()));
    let listen_addr2 = listen_addr1.clone();

    listener_transport.listen_on(listen_addr1).unwrap();

    let server = async move {
        loop {
            let (upgrade, _send_back_addr) =
                match listener_transport.select_next_some().await.into_incoming() {
                    Some(u) => u,
                    None => continue,
                };
            let (peer, _mplex) = upgrade.await.unwrap();
            assert_eq!(peer, dialer_id);
        }
    };

    let client = async move {
        let (peer, _mplex) = dialer_transport.dial(listen_addr2).unwrap().await.unwrap();
        assert_eq!(peer, listener_id);
    };

    async_std::task::spawn(server);
    async_std::task::block_on(client);
}
