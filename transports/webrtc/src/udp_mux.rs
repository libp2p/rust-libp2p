// Copyright 2022 Parity Technologies (UK) Ltd.
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

use async_trait::async_trait;
use futures::StreamExt;
use stun::{
    attributes::ATTR_USERNAME,
    message::{is_message as is_stun_message, Message as STUNMessage},
};
use tokio::{io::ReadBuf, net::UdpSocket};
use tokio_crate as tokio;
use webrtc::ice::udp_mux::{UDPMux, UDPMuxConn, UDPMuxConnParams, UDPMuxWriter};
use webrtc::util::{Conn, Error};

use crate::req_res_chan;
use futures::channel::oneshot;
use futures::future::{BoxFuture, FutureExt, OptionFuture};
use futures::stream::FuturesUnordered;
use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    net::SocketAddr,
    sync::Arc,
    task::{Context, Poll},
};

const RECEIVE_MTU: usize = 8192;

/// A previously unseen address of a remote who've sent us an ICE binding request.
#[derive(Debug)]
pub struct NewAddr {
    pub addr: SocketAddr,
    pub ufrag: String,
}

/// An event emitted by [`UDPMuxNewAddr`] when it's polled.
#[derive(Debug)]
pub enum UDPMuxEvent {
    /// Connection error. UDP mux should be stopped.
    Error(std::io::Error),
    /// Got a [`NewAddr`] from the socket.
    NewAddr(NewAddr),
}

/// A modified version of [`webrtc::ice::udp_mux::UDPMuxDefault`].
///
/// - It has been rewritten to work without locks and channels instead.
/// - It reports previously unseen addresses instead of ignoring them.
pub struct UDPMuxNewAddr {
    udp_sock: UdpSocket,

    /// Maps from ufrag to the underlying connection.
    conns: HashMap<String, UDPMuxConn>,

    /// Maps from socket address to the underlying connection.
    address_map: HashMap<SocketAddr, UDPMuxConn>,

    /// Set of the new addresses to avoid sending the same address multiple times.
    new_addrs: HashSet<SocketAddr>,

    /// `true` when UDP mux is closed.
    is_closed: bool,

    send_buffer: Option<(Vec<u8>, SocketAddr, oneshot::Sender<Result<usize, Error>>)>,

    close_futures: FuturesUnordered<BoxFuture<'static, ()>>,
    write_future: OptionFuture<BoxFuture<'static, ()>>,

    close_command: req_res_chan::Receiver<(), Result<(), Error>>,
    get_conn_command: req_res_chan::Receiver<String, Result<Arc<dyn Conn + Send + Sync>, Error>>,
    remove_conn_command: req_res_chan::Receiver<String, ()>,
    registration_command: req_res_chan::Receiver<(UDPMuxConn, SocketAddr), ()>,
    send_command: req_res_chan::Receiver<(Vec<u8>, SocketAddr), Result<usize, Error>>,

    udp_mux_handle: Arc<UdpMuxHandle>,
    udp_mux_writer_handle: Arc<UdpMuxWriterHandle>,
}

impl UDPMuxNewAddr {
    /// Creates a new UDP muxer.
    pub fn new(udp_sock: UdpSocket) -> Self {
        let (udp_mux_handle, close_command, get_conn_command, remove_conn_command) =
            UdpMuxHandle::new();
        let (udp_mux_writer_handle, registration_command, send_command) = UdpMuxWriterHandle::new();

        Self {
            udp_sock,
            conns: HashMap::default(),
            address_map: HashMap::default(),
            new_addrs: HashSet::default(),
            is_closed: false,
            send_buffer: None,
            close_futures: FuturesUnordered::default(),
            write_future: OptionFuture::default(),
            close_command,
            get_conn_command,
            remove_conn_command,
            registration_command,
            send_command,
            udp_mux_handle: Arc::new(udp_mux_handle),
            udp_mux_writer_handle: Arc::new(udp_mux_writer_handle),
        }
    }

    pub fn udp_mux_handle(&self) -> Arc<UdpMuxHandle> {
        self.udp_mux_handle.clone()
    }

    /// Create a muxed connection for a given ufrag.
    fn create_muxed_conn(&self, ufrag: &str) -> Result<UDPMuxConn, Error> {
        let local_addr = self.udp_sock.local_addr()?;

        let params = UDPMuxConnParams {
            local_addr,
            key: ufrag.into(),
            udp_mux: Arc::downgrade(
                &(self.udp_mux_writer_handle.clone() as Arc<dyn UDPMuxWriter + Send + Sync>),
            ),
        };

        Ok(UDPMuxConn::new(params))
    }

    /// Returns a muxed connection if the `ufrag` from the given STUN message matches an existing
    /// connection.
    fn conn_from_stun_message(&self, buffer: &[u8], addr: &SocketAddr) -> Option<UDPMuxConn> {
        match ufrag_from_stun_message(buffer, true) {
            Ok(ufrag) => self.conns.get(&ufrag).map(Clone::clone),
            Err(e) => {
                log::debug!("{} (addr={})", e, addr);
                None
            }
        }
    }

    /// Reads from the underlying UDP socket and either reports a new address or proxies data to the
    /// muxed connection.
    pub fn poll(&mut self, cx: &mut Context) -> Poll<UDPMuxEvent> {
        loop {
            match self.send_buffer.take() {
                None => {
                    if let Poll::Ready(Some(((buf, target), response))) =
                        self.send_command.poll_next(cx)
                    {
                        self.send_buffer = Some((buf, target, response));
                        continue;
                    }
                }
                Some((buf, target, response)) => {
                    match self.udp_sock.poll_send_to(cx, &buf, target) {
                        Poll::Ready(result) => {
                            let _ = response.send(result.map_err(|e| Error::Io(e.into())));
                            continue;
                        }
                        Poll::Pending => {
                            self.send_buffer = Some((buf, target, response));
                        }
                    }
                }
            }

            if let Poll::Ready(Some(((conn, addr), response))) =
                self.registration_command.poll_next(cx)
            {
                let key = conn.key();

                self.address_map
                    .entry(addr)
                    .and_modify(|e| {
                        if e.key() != key {
                            e.remove_address(&addr);
                            *e = conn.clone();
                        }
                    })
                    .or_insert_with(|| conn.clone());

                // remove addr from new_addrs once conn is established
                self.new_addrs.remove(&addr);

                let _ = response.send(());

                continue;
            }

            if let Poll::Ready(Some((ufrag, response))) = self.get_conn_command.poll_next(cx) {
                if self.is_closed {
                    let _ = response.send(Err(Error::ErrUseClosedNetworkConn));
                    continue;
                }

                if let Some(conn) = self.conns.get(&ufrag).cloned() {
                    let _ = response.send(Ok(Arc::new(conn)));
                    continue;
                }

                let muxed_conn = match self.create_muxed_conn(&ufrag) {
                    Ok(conn) => conn,
                    Err(e) => {
                        let _ = response.send(Err(e));
                        continue;
                    }
                };
                let mut close_rx = muxed_conn.close_rx();

                self.close_futures.push({
                    let ufrag = ufrag.clone();
                    let udp_mux_handle = self.udp_mux_handle.clone();

                    Box::pin(async move {
                        let _ = close_rx.changed().await;
                        udp_mux_handle.remove_conn_by_ufrag(&ufrag).await;
                    })
                });

                self.conns.insert(ufrag, muxed_conn.clone());

                let _ = response.send(Ok(Arc::new(muxed_conn) as Arc<dyn Conn + Send + Sync>));

                continue;
            }

            if let Poll::Ready(Some(((), response))) = self.close_command.poll_next(cx) {
                if self.is_closed {
                    let _ = response.send(Err(Error::ErrAlreadyClosed));
                    continue;
                }

                for (_, conn) in self.conns.drain() {
                    conn.close();
                }

                // NOTE: This is important, we need to drop all instances of `UDPMuxConn` to
                // avoid a retain cycle due to the use of [`std::sync::Arc`] on both sides.
                self.address_map.clear();

                // NOTE: This is important, we need to drop all instances of `UDPMuxConn` to
                // avoid a retain cycle due to the use of [`std::sync::Arc`] on both sides.
                self.new_addrs.clear();

                let _ = response.send(Ok(()));

                self.is_closed = true;

                continue;
            }

            if let Poll::Ready(Some((ufrag, response))) = self.remove_conn_command.poll_next(cx) {
                // Pion's ice implementation has both `RemoveConnByFrag` and `RemoveConn`, but since `conns`
                // is keyed on `ufrag` their implementation is equivalent.

                if let Some(removed_conn) = self.conns.remove(&ufrag) {
                    for address in removed_conn.get_addresses() {
                        self.address_map.remove(&address);
                    }
                }

                let _ = response.send(());

                continue;
            }

            let _ = self.close_futures.poll_next_unpin(cx);

            match self.write_future.poll_unpin(cx) {
                Poll::Ready(Some(())) => {
                    self.write_future = OptionFuture::default();
                    continue;
                }
                Poll::Ready(None) => {
                    // TODO: avoid allocating the buffer each time.
                    let mut recv_buf = [0u8; RECEIVE_MTU];
                    let mut read = ReadBuf::new(&mut recv_buf);

                    match self.udp_sock.poll_recv_from(cx, &mut read) {
                        Poll::Ready(Ok(addr)) => {
                            // Find connection based on previously having seen this source address
                            let conn = self.address_map.get(&addr);

                            let conn = match conn {
                                // If we couldn't find the connection based on source address, see if
                                // this is a STUN mesage and if so if we can find the connection based on ufrag.
                                None if is_stun_message(read.filled()) => {
                                    self.conn_from_stun_message(read.filled(), &addr)
                                }
                                Some(s) => Some(s.to_owned()),
                                _ => None,
                            };

                            match conn {
                                None => {
                                    if !self.new_addrs.contains(&addr) {
                                        match ufrag_from_stun_message(read.filled(), false) {
                                            Ok(ufrag) => {
                                                log::trace!(
                                                "Notifying about new address addr={} from ufrag={}",
                                                &addr,
                                                ufrag
                                            );
                                                self.new_addrs.insert(addr);
                                                return Poll::Ready(UDPMuxEvent::NewAddr(
                                                    NewAddr { addr, ufrag },
                                                ));
                                            }
                                            Err(e) => {
                                                log::debug!(
                                                    "Unknown address addr={} (non STUN packet: {})",
                                                    &addr,
                                                    e
                                                );
                                            }
                                        }
                                    }
                                }
                                Some(conn) => {
                                    let mut packet = vec![0u8; read.filled().len()];
                                    packet.copy_from_slice(read.filled());
                                    self.write_future = OptionFuture::from(Some(
                                        async move {
                                            if let Err(err) = conn.write_packet(&packet, addr).await
                                            {
                                                log::error!(
                                                    "Failed to write packet: {} (addr={})",
                                                    err,
                                                    addr
                                                );
                                            }
                                        }
                                        .boxed(),
                                    ));
                                }
                            }

                            continue;
                        }
                        Poll::Ready(Err(err)) if err.kind() == ErrorKind::TimedOut => {}
                        Poll::Pending => {}
                        Poll::Ready(Err(err)) => {
                            log::error!("Could not read udp packet: {}", err);
                            return Poll::Ready(UDPMuxEvent::Error(err));
                        }
                    }
                }
                Poll::Pending => {}
            }

            return Poll::Pending;
        }
    }
}

pub struct UdpMuxHandle {
    close_sender: req_res_chan::Sender<(), Result<(), Error>>,
    get_conn_sender: req_res_chan::Sender<String, Result<Arc<dyn Conn + Send + Sync>, Error>>,
    remove_sender: req_res_chan::Sender<String, ()>,
}

impl UdpMuxHandle {
    pub fn new() -> (
        Self,
        req_res_chan::Receiver<(), Result<(), Error>>,
        req_res_chan::Receiver<String, Result<Arc<dyn Conn + Send + Sync>, Error>>,
        req_res_chan::Receiver<String, ()>,
    ) {
        let (sender1, receiver1) = req_res_chan::new(1);
        let (sender2, receiver2) = req_res_chan::new(1);
        let (sender3, receiver3) = req_res_chan::new(1);

        let this = Self {
            close_sender: sender1,
            get_conn_sender: sender2,
            remove_sender: sender3,
        };

        (this, receiver1, receiver2, receiver3)
    }
}

#[async_trait]
impl UDPMux for UdpMuxHandle {
    async fn close(&self) -> Result<(), Error> {
        self.close_sender
            .send(())
            .await
            .map_err(|e| Error::Io(e.into()))??;

        Ok(())
    }

    async fn get_conn(self: Arc<Self>, ufrag: &str) -> Result<Arc<dyn Conn + Send + Sync>, Error> {
        let conn = self
            .get_conn_sender
            .send(ufrag.to_owned())
            .await
            .map_err(|e| Error::Io(e.into()))??;

        Ok(conn)
    }

    async fn remove_conn_by_ufrag(&self, ufrag: &str) {
        if let Err(e) = self.remove_sender.send(ufrag.to_owned()).await {
            log::debug!("Failed to send message through channel: {:?}", e);
        }
    }
}

pub struct UdpMuxWriterHandle {
    registration_channel: req_res_chan::Sender<(UDPMuxConn, SocketAddr), ()>,
    send_channel: req_res_chan::Sender<(Vec<u8>, SocketAddr), Result<usize, Error>>,
}

impl UdpMuxWriterHandle {
    fn new() -> (
        Self,
        req_res_chan::Receiver<(UDPMuxConn, SocketAddr), ()>,
        req_res_chan::Receiver<(Vec<u8>, SocketAddr), Result<usize, Error>>,
    ) {
        let (sender1, receiver1) = req_res_chan::new(1);
        let (sender2, receiver2) = req_res_chan::new(1);

        let this = Self {
            registration_channel: sender1,
            send_channel: sender2,
        };

        (this, receiver1, receiver2)
    }
}

#[async_trait]
impl UDPMuxWriter for UdpMuxWriterHandle {
    async fn register_conn_for_address(&self, conn: &UDPMuxConn, addr: SocketAddr) {
        match self
            .registration_channel
            .send((conn.to_owned(), addr))
            .await
        {
            Ok(()) => {}
            Err(e) => {
                log::debug!("Failed to send message through channel: {:?}", e);
                return;
            }
        }

        log::debug!("Registered {} for {}", addr, conn.key());
    }

    async fn send_to(&self, buf: &[u8], target: &SocketAddr) -> Result<usize, Error> {
        let bytes_written = self
            .send_channel
            .send((buf.to_owned(), target.to_owned()))
            .await
            .map_err(|e| Error::Io(e.into()))??;

        Ok(bytes_written)
    }
}

/// Gets the ufrag from the given STUN message or returns an error, if failed to decode or the
/// username attribute is not present.
fn ufrag_from_stun_message(buffer: &[u8], local_ufrag: bool) -> Result<String, Error> {
    let (result, message) = {
        let mut m = STUNMessage::new();

        (m.unmarshal_binary(buffer), m)
    };

    if let Err(err) = result {
        Err(Error::Other(format!(
            "failed to handle decode ICE: {}",
            err
        )))
    } else {
        let (attr, found) = message.attributes.get(ATTR_USERNAME);
        if !found {
            return Err(Error::Other("no username attribute in STUN message".into()));
        }

        match String::from_utf8(attr.value) {
            // Per the RFC this shouldn't happen
            // https://datatracker.ietf.org/doc/html/rfc5389#section-15.3
            Err(err) => Err(Error::Other(format!(
                "failed to decode USERNAME from STUN message as UTF-8: {}",
                err
            ))),
            Ok(s) => {
                // s is a combination of the local_ufrag and the remote ufrag separated by `:`.
                let res = if local_ufrag {
                    s.split(':').next()
                } else {
                    s.split(':').last()
                };
                match res {
                    Some(s) => Ok(s.to_owned()),
                    None => Err(Error::Other("can't get ufrag from username".into())),
                }
            }
        }
    }
}
