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

//! I/O for libp2p-quic.
//!
//! This provides a central location for socket I/O, and logs all incoming and outgoing packets.

use async_std::net::UdpSocket;
use quinn_proto::Transmit;
use std::{future::Future, io::Result, task::Context, task::Poll};
use tracing::{trace, warn};

/// A pending packet for libp2p-quic
#[derive(Debug, Default)]
pub(crate) struct Pending {
    pending: Option<Transmit>,
}

/// A socket wrapper for libp2p-quic
#[derive(Debug)]
pub(crate) struct Socket {
    socket: UdpSocket,
}

impl Socket {
    /// Transmit a packet if possible, with appropriate logging.
    ///
    /// We ignore I/O errors. If a packet cannot be sent, we assume it is a transient condition and
    /// drop it. If it is not, the connection will eventually time out. This provides a very high
    /// degree of robustness. Connections will transparently resume after a transient network
    /// outage, and problems that are specific to one peer will not effect other peers.
    pub(crate) fn poll_send_to(&self, cx: &mut Context<'_>, packet: &Transmit) -> Poll<Result<()>> {
        match {
            let fut = self.socket.send_to(&packet.contents, &packet.destination);
            futures::pin_mut!(fut);
            fut.poll(cx)
        } {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(e)) => {
                trace!("sent packet of length {} to {}", e, packet.destination);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => {
                warn!("Ignoring I/O error on transmit: {:?}", e);
                Poll::Ready(Ok(()))
            }
        }
    }

    /// A wrapper around `recv_from` that handles ECONNRESET and logging
    pub(crate) fn recv_from(
        &self,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<(usize, std::net::SocketAddr)>> {
        loop {
            match {
                let fut = self.socket.recv_from(buf);
                futures::pin_mut!(fut);
                fut.poll(cx)
            } {
                Poll::Pending => {
                    break Poll::Pending;
                }
                Poll::Ready(Ok(e)) => {
                    trace!("received packet of length {} from {}", e.0, e.1);
                    break Poll::Ready(Ok(e));
                }
                // May be injected by a malicious ICMP packet
                Poll::Ready(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                    warn!("Got ECONNRESET from recv_from() - possible MITM attack")
                }
                Poll::Ready(Err(e)) => {
                    warn!("Fatal I/O error: {:?}", e);
                    break Poll::Ready(Err(e));
                }
            }
        }
    }

    pub(crate) fn send_packets(
        &self,
        cx: &mut Context<'_>,
        pending: &mut Pending,
        source: &mut dyn FnMut() -> Option<Transmit>,
    ) -> Poll<Result<()>> {
        if let Some(ref mut transmit) = pending.pending {
            trace!("trying to send packet!");
            match self.poll_send_to(cx, &transmit) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            }
        }
        pending.pending = None;
        while let Some(transmit) = source() {
            trace!("trying to send packet!");
            match self.poll_send_to(cx, &transmit) {
                Poll::Ready(Ok(())) => {}
                Poll::Pending => {
                    pending.pending = Some(transmit);
                    return Poll::Pending;
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl Socket {
    pub(crate) fn new(socket: UdpSocket) -> Self {
        Self { socket }
    }
}
