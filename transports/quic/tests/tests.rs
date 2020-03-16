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

use async_macros::ready;
use futures::prelude::*;
use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::ListenerEvent,
    StreamMuxer, Transport,
};
use libp2p_quic::{Config, Endpoint, Muxer, Substream};
use log::{debug, info, trace};
use std::{
    io::Result,
    pin::Pin,
    task::{Context, Poll},
};

#[derive(Debug)]
struct QuicStream {
    id: Option<Substream>,
    muxer: Muxer,
    shutdown: bool,
}

impl AsyncWrite for QuicStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize>> {
        assert!(!self.shutdown, "written after close");
        let Self { muxer, id, .. } = self.get_mut();
        muxer
            .write_substream(cx, id.as_mut().unwrap(), buf)
            .map_err(From::from)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        self.shutdown = true;
        let Self { muxer, id, .. } = self.get_mut();
        debug!("trying to close {:?}", id);
        ready!(muxer.shutdown_substream(cx, id.as_mut().unwrap()))?;
        debug!("closed {:?}", id);
        Poll::Ready(Ok(()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for QuicStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize>> {
        let Self { id, muxer, .. } = self.get_mut();
        muxer
            .read_substream(cx, id.as_mut().unwrap(), buf)
            .map_err(From::from)
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

struct Outbound<'a>(&'a mut Muxer, libp2p_quic::Outbound);

impl<'a> futures::Future for Outbound<'a> {
    type Output = Result<QuicStream>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Outbound(conn, id) = &mut *self;
        let id: Option<Substream> = Some(ready!(conn.poll_outbound(cx, id))?);
        Poll::Ready(Ok(QuicStream {
            id,
            muxer: self.get_mut().0.clone(),
            shutdown: false,
        }))
    }
}

#[derive(Debug)]
struct Inbound<'a>(&'a mut Muxer);
impl<'a> futures::Stream for Inbound<'a> {
    type Item = QuicStream;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(Some(QuicStream {
            id: Some(ready!(self.0.poll_inbound(cx)).expect("bug")),
            muxer: self.get_mut().0.clone(),
            shutdown: false,
        }))
    }
}

fn init() {
    use tracing_subscriber::{fmt::Subscriber, EnvFilter};
    let _ = Subscriber::builder()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();
}

struct Closer(Muxer);

impl Future for Closer {
    type Output = Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.get_mut().0.close(cx).map_err(From::from)
    }
}

#[test]
fn wildcard_expansion() {
    init();
    let addr: Multiaddr = "/ip4/0.0.0.0/udp/1234/quic".parse().unwrap();
    let keypair = libp2p_core::identity::Keypair::generate_ed25519();
    let (listener, join) = Endpoint::new(Config::new(&keypair, addr.clone())).unwrap();
    let mut incoming = listener.listen_on(addr).unwrap();
    // Process all initial `NewAddress` events and make sure they
    // do not contain wildcard address or port.
    futures::executor::block_on(async move {
        while let Some(event) = incoming.next().await.map(|e| e.unwrap()) {
            match event {
                ListenerEvent::NewAddress(a) => {
                    let mut iter = a.iter();
                    match iter.next().expect("ip address") {
                        Protocol::Ip4(ip) => assert!(!ip.is_unspecified()),
                        Protocol::Ip6(ip) => assert!(!ip.is_unspecified()),
                        other => panic!("Unexpected protocol: {}", other),
                    }
                    if let Protocol::Udp(port) = iter.next().expect("port") {
                        assert_ne!(0, port)
                    } else {
                        panic!("No UDP port in address: {}", a)
                    }
                    assert_eq!(iter.next(), Some(Protocol::Quic));
                    assert_eq!(iter.next(), None);
                }
                ListenerEvent::Upgrade { .. } => panic!(),
                ListenerEvent::AddressExpired { .. } => panic!(),
                ListenerEvent::Error { .. } => panic!(),
            }
            break;
        }
        drop(incoming);
        join.await.unwrap()
    });
}

#[test]
fn communicating_between_dialer_and_listener() {
    use tracing::info_span;
    init();
    for i in 0..1000u32 {
        let span = info_span!("test", id = i);
        let _guard = span.enter();
        do_test()
    }
}

fn do_test() {
    use std::error::Error as _;
    let (ready_tx, ready_rx) = futures::channel::oneshot::channel();
    let mut ready_tx = Some(ready_tx);
    let keypair = libp2p_core::identity::Keypair::generate_ed25519();
    let keypair2 = keypair.clone();
    let addr: Multiaddr = "/ip4/127.0.0.1/udp/0/quic".parse().expect("bad address?");
    let quic_config = Config::new(&keypair2, addr.clone());
    let (quic_endpoint, join) = Endpoint::new(quic_config).unwrap();
    let mut listener = quic_endpoint.clone().listen_on(addr).unwrap();
    trace!("running tests");
    let handle = async_std::task::spawn(async move {
        let key = loop {
            trace!("awaiting connection");
            match listener.next().await.unwrap().unwrap() {
                ListenerEvent::NewAddress(listen_addr) => {
                    if let Some(channel) = ready_tx.take() {
                        channel.send(listen_addr).unwrap();
                    }
                }
                ListenerEvent::Upgrade { upgrade, .. } => {
                    debug!("got a connection upgrade!");
                    let (id, mut muxer): (_, Muxer) = upgrade.await.expect("upgrade failed");
                    debug!("got a new muxer!");
                    let mut socket: QuicStream = Inbound(&mut muxer)
                        .next()
                        .await
                        .expect("no incoming stream");

                    let mut buf = [0u8; 3];
                    debug!("reading data from accepted stream!");
                    {
                        let mut count = 0;
                        while count < buf.len() {
                            count += socket.read(&mut buf[count..]).await.unwrap();
                        }
                    }
                    assert_eq!(buf, [4, 5, 6]);
                    debug!("writing data!");
                    socket.write_all(&[0x1, 0x2, 0x3]).await.unwrap();
                    debug!("data written!");
                    // socket.close().await.unwrap();
                    // debug!("socket closed!");
                    assert_eq!(socket.read(&mut buf).await.unwrap(), 0);
                    debug!("end of stream");
                    drop(socket);
                    Closer(muxer).await.unwrap();
                    debug!("finished!");
                    break id;
                }
                _ => unreachable!(),
            }
        };
        drop(listener);
        drop(quic_endpoint);
        join.await.unwrap();
        key
    });

    let second_handle = async_std::task::spawn(async move {
        let addr = ready_rx.await.unwrap();
        let quic_config = Config::new(&keypair, "/ip4/127.0.0.1/udp/0/quic".parse().unwrap());
        let (quic_endpoint, join) =
            Endpoint::new(quic_config).unwrap();
        // Obtain a future socket through dialing
        let mut connection = quic_endpoint.dial(addr.clone()).unwrap().await.unwrap();
        trace!("Received a Connection: {:?}", connection);
        let id = connection.1.open_outbound();
        let mut stream = Outbound(&mut connection.1, id).await.expect("failed");

        debug!("opened a stream: id {:?}", stream.id);
        let result = stream.read(&mut [][..]).await;
        let result = result.expect_err("reading from an unwritten stream cannot succeed");
        assert_eq!(result.kind(), std::io::ErrorKind::NotConnected);
        assert!(result.source().is_none());
        let wrapped = result.get_ref().unwrap().downcast_ref().unwrap();
        match wrapped {
            libp2p_quic::Error::CannotReadFromUnwrittenStream => {}
            e => panic!("Wrong error from reading unwritten stream: {}", e),
        }
        stream.write_all(&[4u8, 5, 6]).await.unwrap();
        stream.close().await.unwrap();
        let mut buf = [0u8; 3];
        debug!("reading data!");
        {
            let mut count = 0;
            while count < buf.len() {
                let read = stream.read(&mut buf[count..]).await.unwrap();
                assert_ne!(read, 0usize, "premature end of file");
                count += read;
            }
        }
        assert_eq!(buf, [1u8, 2, 3]);
        debug!("data read ― checking for EOF");
        assert_eq!(stream.read(&mut buf).await.unwrap(), 0);
        drop(stream);
        debug!("have EOF");
        Closer(connection.1).await.expect("closed successfully");
        debug!("awaiting handle");
        join.await.unwrap();
        info!("endpoint is finished");
        connection.0
    });
    assert_eq!(
        async_std::task::block_on(handle),
        async_std::task::block_on(second_handle)
    );
}

#[test]
fn replace_port_0_in_returned_multiaddr_ipv4() {
    init();
    let keypair = libp2p_core::identity::Keypair::generate_ed25519();

    let addr = "/ip4/127.0.0.1/udp/0/quic".parse::<Multiaddr>().unwrap();
    assert!(addr.to_string().ends_with("udp/0/quic"));

    let config = Config::new(&keypair, addr.clone());
    let (quic, join) = Endpoint::new(config).expect("no error");

    let new_addr = futures::executor::block_on_stream(quic.listen_on(addr).unwrap())
        .next()
        .expect("some event")
        .expect("no error")
        .into_new_address()
        .expect("listen address");

    if new_addr.to_string().contains("udp/0") {
        panic!("failed to expand address ― got {}", new_addr);
    }
    futures::executor::block_on(join).unwrap()
}

#[test]
fn replace_port_0_in_returned_multiaddr_ipv6() {
    init();
    let keypair = libp2p_core::identity::Keypair::generate_ed25519();

    let addr: Multiaddr = "/ip6/::1/udp/0/quic".parse().unwrap();
    assert!(addr.to_string().contains("udp/0/quic"));
    let config = Config::new(&keypair, addr.clone());
    let (quic, join) = Endpoint::new(config).expect("no error");

    let new_addr = futures::executor::block_on_stream(quic.listen_on(addr).unwrap())
        .next()
        .expect("some event")
        .expect("no error")
        .into_new_address()
        .expect("listen address");

    assert!(!new_addr.to_string().contains("udp/0"));
    futures::executor::block_on(join).unwrap()
}

#[test]
fn larger_addr_denied() {
    init();
    let keypair = libp2p_core::identity::Keypair::generate_ed25519();
    let addr = "/ip4/127.0.0.1/tcp/12345/tcp/12345"
        .parse::<Multiaddr>()
        .unwrap();
        let config = Config::new(&keypair, addr);
        assert!(Endpoint::new(config).is_err())
}
