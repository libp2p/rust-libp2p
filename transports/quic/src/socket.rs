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
use log::{debug, trace};
use quinn_proto::{Connection, Endpoint, Transmit};
use std::{io::Result, task::Context, task::Poll, time::Instant};

mod private {
    use super::Instant;
    pub(crate) struct Dummy;
    /// A trait for packet generators
    pub(crate) trait PacketGen {
        /// Get the next packet to be sent, if any.
        fn get_packet(&mut self, dummy: Dummy, now: Instant) -> Option<quinn_proto::Transmit>;
    }
}

use private::Dummy;
pub(crate) use private::PacketGen;

impl PacketGen for Endpoint {
    fn get_packet(&mut self, Dummy: Dummy, _now: Instant) -> Option<quinn_proto::Transmit> {
        self.poll_transmit()
    }
}

impl PacketGen for Connection {
    fn get_packet(&mut self, Dummy: Dummy, now: Instant) -> Option<quinn_proto::Transmit> {
        self.poll_transmit(now)
    }
}

/// A socket wrapper for libp2p-quic
#[derive(Debug, Default)]
pub(crate) struct Socket {
    pending: Option<Transmit>,
}

impl Socket {
    pub fn send_packet(
        &mut self,
        cx: &mut Context,
        socket: &UdpSocket,
        source: &mut dyn PacketGen,
        now: Instant,
    ) -> Poll<Result<()>> {
        if let Some(ref mut transmit) = self.pending {
            trace!("trying to send packet!");
            match socket.poll_send_to(cx, &transmit.contents, &transmit.destination) {
                Poll::Pending => {
                    debug!("not able to send packet right away");
                    return Poll::Pending;
                }
                Poll::Ready(Ok(_)) => {}
                // FIXME is this even possible?
                Poll::Ready(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {}
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            }
            trace!(
                "sent packet of length {} to {}",
                transmit.contents.len(),
                transmit.destination
            );
        }
        self.pending = None;
        while let Some(transmit) = source.get_packet(Dummy, now) {
            trace!("trying to send packet!");
            match socket.poll_send_to(cx, &transmit.contents, &transmit.destination) {
                Poll::Pending => debug!("not able to send packet right away"),
                Poll::Ready(Ok(_)) => {
                    trace!(
                        "sent packet of length {} to {}",
                        transmit.contents.len(),
                        transmit.destination
                    );
                    continue;
                }
                // FIXME is this even possible?
                Poll::Ready(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => continue,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            }
            self.pending = Some(transmit);
            return Poll::Pending;
        }
        Poll::Ready(Ok(()))
    }
}
