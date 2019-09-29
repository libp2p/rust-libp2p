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

mod util;

use futures::future::Future;
use futures::stream::Stream;
use libp2p_core::identity;
use libp2p_core::transport::{Transport, MemoryTransport, ListenerEvent};
use libp2p_core::upgrade::{self, UpgradeInfo, Negotiated, InboundUpgrade, OutboundUpgrade};
use libp2p_mplex::MplexConfig;
use libp2p_secio::SecioConfig;
use multiaddr::Multiaddr;
use rand::random;
use std::io;
use tokio_io::{io as nio, AsyncWrite, AsyncRead};

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
    C: AsyncRead + AsyncWrite + Send + 'static
{
    type Output = Negotiated<C>;
    type Error = io::Error;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_inbound(self, socket: Negotiated<C>, _: Self::Info) -> Self::Future {
        Box::new(nio::read_exact(socket, [0u8; 5]).map(|(io, buf)| {
            assert_eq!(&buf[..], "hello".as_bytes());
            io
        }))
    }
}

impl<C> OutboundUpgrade<C> for HelloUpgrade
where
    C: AsyncWrite + AsyncRead + Send + 'static,
{
    type Output = Negotiated<C>;
    type Error = io::Error;
    type Future = Box<dyn Future<Item = Self::Output, Error = Self::Error> + Send>;

    fn upgrade_outbound(self, socket: Negotiated<C>, _: Self::Info) -> Self::Future {
        Box::new(nio::write_all(socket, "hello").map(|(io, _)| io))
    }
}

#[test]
fn upgrade_pipeline() {
    let listener_keys = identity::Keypair::generate_ed25519();
    let listener_id = listener_keys.public().into_peer_id();
    let listener_transport = MemoryTransport::default()
        .upgrade(upgrade::Version::V1)
        .authenticate(SecioConfig::new(listener_keys))
        .apply(HelloUpgrade {})
        .apply(HelloUpgrade {})
        .apply(HelloUpgrade {})
        .multiplex(MplexConfig::default())
        .and_then(|(peer, mplex), _| {
            // Gracefully close the connection to allow protocol
            // negotiation to complete.
            util::CloseMuxer::new(mplex).map(move |mplex| (peer, mplex))
        });

    let dialer_keys = identity::Keypair::generate_ed25519();
    let dialer_id = dialer_keys.public().into_peer_id();
    let dialer_transport = MemoryTransport::default()
        .upgrade(upgrade::Version::V1)
        .authenticate(SecioConfig::new(dialer_keys))
        .apply(HelloUpgrade {})
        .apply(HelloUpgrade {})
        .apply(HelloUpgrade {})
        .multiplex(MplexConfig::default())
        .and_then(|(peer, mplex), _| {
            // Gracefully close the connection to allow protocol
            // negotiation to complete.
            util::CloseMuxer::new(mplex).map(move |mplex| (peer, mplex))
        });

    let listen_addr: Multiaddr = format!("/memory/{}", random::<u64>()).parse().unwrap();
    let listener = listener_transport.listen_on(listen_addr.clone()).unwrap()
        .filter_map(ListenerEvent::into_upgrade)
        .for_each(move |(upgrade, _remote_addr)| {
            let dialer = dialer_id.clone();
            upgrade.map(move |(peer, _mplex)| {
                assert_eq!(peer, dialer)
            })
        })
        .map_err(|e| panic!("Listener error: {}", e));

    let dialer = dialer_transport.dial(listen_addr).unwrap()
        .map(move |(peer, _mplex)| {
            assert_eq!(peer, listener_id)
        });

    let mut rt = tokio::runtime::Runtime::new().unwrap();
    rt.spawn(listener);
    rt.block_on(dialer).unwrap()
}

