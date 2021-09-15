// Copyright 2020 Parity Technologies (UK) Ltd.
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

use super::{IfEvent, Incoming, Provider};

use futures::{
    future::{self, BoxFuture, FutureExt},
    prelude::*,
};
use futures_timer::Delay;
use if_addrs::{get_if_addrs, IfAddr};
use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use std::collections::HashSet;
use std::convert::TryFrom;
use std::io;
use std::net;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

#[derive(Copy, Clone)]
pub enum Tcp {}

pub struct IfWatcher {
    addrs: HashSet<IpNet>,
    delay: Delay,
    pending: Vec<IfEvent>,
}

impl Provider for Tcp {
    type Stream = TcpStream;
    type Listener = tokio_crate::net::TcpListener;
    type IfWatcher = IfWatcher;

    fn if_watcher() -> BoxFuture<'static, io::Result<Self::IfWatcher>> {
        future::ready(Ok(IfWatcher {
            addrs: HashSet::new(),
            delay: Delay::new(Duration::from_secs(0)),
            pending: Vec::new(),
        }))
        .boxed()
    }

    fn new_listener(l: net::TcpListener) -> io::Result<Self::Listener> {
        tokio_crate::net::TcpListener::try_from(l)
    }

    fn new_stream(s: net::TcpStream) -> BoxFuture<'static, io::Result<Self::Stream>> {
        async move {
            let stream = tokio_crate::net::TcpStream::try_from(s)?;
            stream.writable().await?;
            Ok(TcpStream(stream))
        }
        .boxed()
    }

    fn poll_accept(
        l: &mut Self::Listener,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<Incoming<Self::Stream>>> {
        let (stream, remote_addr) = match l.poll_accept(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Ready(Ok((stream, remote_addr))) => (stream, remote_addr),
        };

        let local_addr = stream.local_addr()?;
        let stream = TcpStream(stream);

        Poll::Ready(Ok(Incoming {
            stream,
            local_addr,
            remote_addr,
        }))
    }

    fn poll_interfaces(w: &mut Self::IfWatcher, cx: &mut Context<'_>) -> Poll<io::Result<IfEvent>> {
        loop {
            if let Some(event) = w.pending.pop() {
                return Poll::Ready(Ok(event));
            }

            match Pin::new(&mut w.delay).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(()) => {
                    let ifs = get_if_addrs()?;
                    let addrs = ifs
                        .into_iter()
                        .map(|iface| match iface.addr {
                            IfAddr::V4(ip4) => {
                                let prefix_len =
                                    (!u32::from_be_bytes(ip4.netmask.octets())).leading_zeros();
                                let ipnet = Ipv4Net::new(ip4.ip, prefix_len as u8)
                                    .expect("prefix_len can not exceed 32");
                                IpNet::V4(ipnet)
                            }
                            IfAddr::V6(ip6) => {
                                let prefix_len =
                                    (!u128::from_be_bytes(ip6.netmask.octets())).leading_zeros();
                                let ipnet = Ipv6Net::new(ip6.ip, prefix_len as u8)
                                    .expect("prefix_len can not exceed 128");
                                IpNet::V6(ipnet)
                            }
                        })
                        .collect::<HashSet<_>>();

                    for down in w.addrs.difference(&addrs) {
                        w.pending.push(IfEvent::Down(*down));
                    }

                    for up in addrs.difference(&w.addrs) {
                        w.pending.push(IfEvent::Up(*up));
                    }

                    w.addrs = addrs;
                    w.delay.reset(Duration::from_secs(10));
                }
            }
        }
    }
}

/// A [`tokio_crate::net::TcpStream`] that implements [`AsyncRead`] and [`AsyncWrite`].
#[derive(Debug)]
pub struct TcpStream(pub tokio_crate::net::TcpStream);

impl Into<tokio_crate::net::TcpStream> for TcpStream {
    fn into(self: TcpStream) -> tokio_crate::net::TcpStream {
        self.0
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut [u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut read_buf = tokio_crate::io::ReadBuf::new(buf);
        futures::ready!(tokio_crate::io::AsyncRead::poll_read(
            Pin::new(&mut self.0),
            cx,
            &mut read_buf
        ))?;
        Poll::Ready(Ok(read_buf.filled().len()))
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        tokio_crate::io::AsyncWrite::poll_write(Pin::new(&mut self.0), cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        tokio_crate::io::AsyncWrite::poll_flush(Pin::new(&mut self.0), cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        tokio_crate::io::AsyncWrite::poll_shutdown(Pin::new(&mut self.0), cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        tokio_crate::io::AsyncWrite::poll_write_vectored(Pin::new(&mut self.0), cx, bufs)
    }
}
