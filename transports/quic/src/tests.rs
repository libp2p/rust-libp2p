// Copyright 2017-2020 Parity Technologies (UK) Ltd.
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

use super::*;
use futures::prelude::*;
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::ListenerEvent,
    Transport,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[derive(Debug)]
pub struct QuicStream {
    id: Option<QuicSubstream>,
    muxer: QuicMuxer,
}

impl AsyncWrite for QuicStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let inner = self.get_mut();
        inner.muxer.write_substream(
            cx,
            &mut inner.id.as_mut().expect("written to after being closed"),
            buf,
        )
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        debug!("trying to close");
        let inner = self.get_mut();
        inner.muxer.shutdown_substream(
            cx,
            inner.id.as_mut().expect("written to after being closed"),
        )
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Result<(), io::Error>> {
        Ready(Ok(()))
    }
}

impl AsyncRead for QuicStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let inner = self.get_mut();
        inner
            .muxer
            .read_substream(cx, &mut inner.id.as_mut().expect("read after close"), buf)
    }
}

impl Drop for QuicStream {
    fn drop(&mut self) {
        match self.id.take() {
            None => {}
            Some(id) => self.muxer.destroy_substream(id),
        }
    }
}

impl futures::Stream for QuicMuxer {
    type Item = QuicStream;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.poll_inbound(cx).map(|x| match x {
            Ok(id) => Some(QuicStream {
                id: Some(id),
                muxer: self.get_mut().clone(),
            }),
            Err(_) => None,
        })
    }
}

fn init() {
    drop(env_logger::try_init());
}

impl Future for QuicMuxer {
    type Output = Result<(), io::Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.get_mut().close(cx)
    }
}

#[test]
fn wildcard_expansion() {
    init();
    let addr: Multiaddr = "/ip4/0.0.0.0/udp/1234/quic".parse().unwrap();
    let listener = QuicEndpoint::new(&QuicConfig::default(), addr.clone())
        .expect("endpoint")
        .listen_on(addr)
        .expect("listener");
    let addr: Multiaddr = "/ip4/127.0.0.1/udp/1236/quic".parse().unwrap();
    let client = QuicEndpoint::new(&QuicConfig::default(), addr.clone())
        .expect("endpoint")
        .dial(addr)
        .expect("dialer");

    // Process all initial `NewAddress` events and make sure they
    // do not contain wildcard address or port.
    let server = listener
        .take_while(|event| match event.as_ref().unwrap() {
            ListenerEvent::NewAddress(a) => {
                let mut iter = a.iter();
                match iter.next().expect("ip address") {
                    Protocol::Ip4(_ip) => {} // assert!(!ip.is_unspecified()),
                    Protocol::Ip6(_ip) => {} // assert!(!ip.is_unspecified()),
                    other => panic!("Unexpected protocol: {}", other),
                }
                if let Protocol::Udp(port) = iter.next().expect("port") {
                    assert_ne!(0, port)
                } else {
                    panic!("No UDP port in address: {}", a)
                }
                futures::future::ready(true)
            }
            _ => futures::future::ready(false),
        })
        .for_each(|_| futures::future::ready(()));

    async_std::task::spawn(server);
    futures::executor::block_on(client).unwrap();
}

#[test]
fn multiaddr_to_udp_conversion() {
    use std::net::Ipv6Addr;
    init();
    assert!(
        multiaddr_to_socketaddr(&"/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap()).is_err()
    );

    assert!(
        multiaddr_to_socketaddr(&"/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().unwrap()).is_err()
    );

    assert_eq!(
        multiaddr_to_socketaddr(
            &"/ip4/127.0.0.1/udp/12345/quic"
                .parse::<Multiaddr>()
                .unwrap()
        ),
        Ok(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            12345,
        ))
    );
    assert_eq!(
        multiaddr_to_socketaddr(
            &"/ip4/255.255.255.255/udp/8080/quic"
                .parse::<Multiaddr>()
                .unwrap()
        ),
        Ok(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)),
            8080,
        ))
    );
    assert_eq!(
        multiaddr_to_socketaddr(&"/ip6/::1/udp/12345/quic".parse::<Multiaddr>().unwrap()),
        Ok(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
            12345,
        ))
    );
    assert_eq!(
        multiaddr_to_socketaddr(
            &"/ip6/ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff/udp/8080/quic"
                .parse::<Multiaddr>()
                .unwrap()
        ),
        Ok(SocketAddr::new(
            IpAddr::V6(Ipv6Addr::new(
                65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535,
            )),
            8080,
        ))
    );
}

#[test]
fn communicating_between_dialer_and_listener() {
    use super::{trace, StreamMuxer};
    init();
    let (ready_tx, ready_rx) = futures::channel::oneshot::channel();
    let mut ready_tx = Some(ready_tx);

    let _handle = async_std::task::spawn(async move {
        let addr: Multiaddr = "/ip4/127.0.0.1/udp/12345/quic"
            .parse()
            .expect("bad address?");
        let quic_config = QuicConfig::default();
        let quic_endpoint = QuicEndpoint::new(&quic_config, addr.clone()).expect("I/O error");
        let mut listener = quic_endpoint.listen_on(addr).unwrap();

        loop {
            trace!("awaiting connection");
            match listener.next().await.unwrap().unwrap() {
                ListenerEvent::NewAddress(listen_addr) => {
                    ready_tx.take().unwrap().send(listen_addr).unwrap();
                }
                ListenerEvent::Upgrade { upgrade, .. } => {
                    let mut muxer: QuicMuxer = upgrade.await.expect("upgrade failed");
                    let mut socket: QuicStream = muxer.next().await.expect("no incoming stream");

                    let mut buf = [0u8; 3];
                    log::error!("reading data from accepted stream!");
                    socket.read_exact(&mut buf).await.unwrap();
                    assert_eq!(buf, [4, 5, 6]);
                    log::error!("writing data!");
                    socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();
                    log::error!("data written!");
                    socket.close().await.unwrap();
                    assert_eq!(socket.read(&mut buf).await.unwrap(), 0);
                    drop(socket);
                    log::error!("end of stream");
                    muxer.await.unwrap();
                    break;
                }
                _ => unreachable!(),
            }
        }
    });

    async_std::task::block_on(async move {
        let addr = ready_rx.await.unwrap();
        let quic_config = QuicConfig::default();
        let quic_endpoint = QuicEndpoint::new(
            &quic_config,
            "/ip4/127.0.0.1/udp/12346/quic".parse().unwrap(),
        )
        .unwrap();
        // Obtain a future socket through dialing
        let connection = quic_endpoint.dial(addr.clone()).unwrap().await.unwrap();
        trace!("Received a Connection: {:?}", connection);
        let mut stream = QuicStream {
            id: Some(connection.open_outbound().await.expect("failed")),
            muxer: connection.clone(),
        };
        log::warn!("have a new stream!");
        stream.write_all(&[4u8, 5, 6]).await.unwrap();
        let mut buf = [0u8; 3];
        log::error!("reading data!");
        stream.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, [1u8, 2, 3]);
        log::error!("data read!");
        stream.close().await.unwrap();
        assert_eq!(stream.read(&mut buf).await.unwrap(), 0);
        drop(stream);
        connection.await.unwrap();
        log::error!("awaiting handle!");
        _handle.await;
    });
}

#[test]
fn replace_port_0_in_returned_multiaddr_ipv4() {
    init();
    let quic = QuicConfig::default();

    let addr = "/ip4/127.0.0.1/udp/0/quic".parse::<Multiaddr>().unwrap();
    assert!(addr.to_string().ends_with("udp/0/quic"));

    let quic = QuicEndpoint::new(&quic, addr.clone()).expect("no error");

    let new_addr = futures::executor::block_on_stream(quic.listen_on(addr).unwrap())
        .next()
        .expect("some event")
        .expect("no error")
        .into_new_address()
        .expect("listen address");

    assert!(!new_addr.to_string().contains("tcp/0"));
}

#[test]
fn replace_port_0_in_returned_multiaddr_ipv6() {
    init();
    let config = QuicConfig::default();

    let addr: Multiaddr = "/ip6/::1/udp/0/quic".parse().unwrap();
    assert!(addr.to_string().contains("udp/0/quic"));
    let quic = QuicEndpoint::new(&config, addr.clone()).expect("no error");

    let new_addr = futures::executor::block_on_stream(quic.listen_on(addr).unwrap())
        .next()
        .expect("some event")
        .expect("no error")
        .into_new_address()
        .expect("listen address");

    assert!(!new_addr.to_string().contains("tcp/0"));
}

#[test]
fn larger_addr_denied() {
    init();
    let config = QuicConfig::default();
    let addr = "/ip4/127.0.0.1/tcp/12345/tcp/12345"
        .parse::<Multiaddr>()
        .unwrap();
    assert!(QuicEndpoint::new(&config, addr).is_err())
}
