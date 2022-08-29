// MIT License
//
// Copyright (c) 2021 WebRTC.rs
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use async_trait::async_trait;
use futures::ready;
use stun::{
    attributes::ATTR_USERNAME,
    message::{is_message as is_stun_message, Message as STUNMessage},
};
use tokio::{io::ReadBuf, net::UdpSocket};
use tokio_crate as tokio;
use webrtc::ice::udp_mux::{UDPMux, UDPMuxConn, UDPMuxConnParams, UDPMuxWriter};
use webrtc::util::{sync::RwLock, Conn, Error};

use std::{
    collections::{HashMap, HashSet},
    io::ErrorKind,
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
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

/// A modified version of [`webrtc_ice::udp_mux::UDPMuxDefault`], which reports previously unseen
/// addresses instead of ignoring them.
pub struct UDPMuxNewAddr {
    udp_sock: UdpSocket,

    /// Maps from ufrag to the underlying connection.
    conns: Mutex<HashMap<String, UDPMuxConn>>,

    /// Maps from socket address to the underlying connection.
    address_map: RwLock<HashMap<SocketAddr, UDPMuxConn>>,

    /// Set of the new addresses to avoid sending the same address multiple times.
    new_addrs: RwLock<HashSet<SocketAddr>>,

    /// `true` when UDP mux is closed.
    is_closed: AtomicBool,
}

impl UDPMuxNewAddr {
    /// Creates a new UDP muxer.
    pub fn new(udp_sock: UdpSocket) -> Arc<Self> {
        Arc::new(Self {
            udp_sock,
            conns: Mutex::default(),
            address_map: RwLock::default(),
            new_addrs: RwLock::default(),
            is_closed: AtomicBool::new(false),
        })
    }

    /// Create a muxed connection for a given ufrag.
    fn create_muxed_conn(self: &Arc<Self>, ufrag: &str) -> Result<UDPMuxConn, Error> {
        let local_addr = self.udp_sock.local_addr()?;

        let params = UDPMuxConnParams {
            local_addr,
            key: ufrag.into(),
            udp_mux: Arc::downgrade(self) as Weak<dyn UDPMuxWriter + Sync + Send>,
        };

        Ok(UDPMuxConn::new(params))
    }

    /// Returns a muxed connection if the `ufrag` from the given STUN message matches an existing
    /// connection.
    fn conn_from_stun_message(&self, buffer: &[u8], addr: &SocketAddr) -> Option<UDPMuxConn> {
        match ufrag_from_stun_message(buffer, true) {
            Ok(ufrag) => {
                let conns = self.conns.lock().unwrap();
                conns.get(&ufrag).map(Clone::clone)
            }
            Err(e) => {
                log::debug!("{} (addr={})", e, addr);
                None
            }
        }
    }

    /// Returns true if the UDP muxer is closed.
    pub fn is_closed(&self) -> bool {
        return self.is_closed.load(Ordering::Relaxed);
    }

    /// Reads from the underlying UDP socket and either reports a new address or proxies data to the
    /// muxed connection.
    pub fn poll(&self, cx: &mut Context) -> Poll<UDPMuxEvent> {
        // TODO: avoid allocating the buffer each time.
        let mut recv_buf = [0u8; RECEIVE_MTU];
        let mut read = ReadBuf::new(&mut recv_buf);

        loop {
            match ready!(self.udp_sock.poll_recv_from(cx, &mut read)) {
                Ok(addr) => {
                    // Find connection based on previously having seen this source address
                    let conn = {
                        let address_map = self.address_map.read();
                        address_map.get(&addr).map(Clone::clone)
                    };

                    let conn = match conn {
                        // If we couldn't find the connection based on source address, see if
                        // this is a STUN mesage and if so if we can find the connection based on ufrag.
                        None if is_stun_message(read.filled()) => {
                            self.conn_from_stun_message(&read.filled(), &addr)
                        }
                        s @ Some(_) => s,
                        _ => None,
                    };

                    match conn {
                        None => {
                            if !self.new_addrs.read().contains(&addr) {
                                match ufrag_from_stun_message(read.filled(), false) {
                                    Ok(ufrag) => {
                                        log::trace!(
                                            "Notifying about new address addr={} from ufrag={}",
                                            &addr,
                                            ufrag
                                        );
                                        let mut new_addrs = self.new_addrs.write();
                                        new_addrs.insert(addr);
                                        return Poll::Ready(UDPMuxEvent::NewAddr(NewAddr {
                                            addr,
                                            ufrag,
                                        }));
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
                            write_packet_to_conn_from_addr(conn, packet, addr);
                        }
                    }
                }
                Err(err) if err.kind() == ErrorKind::TimedOut => {}
                Err(err) => {
                    log::error!("Could not read udp packet: {}", err);
                    return Poll::Ready(UDPMuxEvent::Error(err));
                }
            }
        }
    }
}

fn write_packet_to_conn_from_addr(conn: UDPMuxConn, packet: Vec<u8>, addr: SocketAddr) {
    // Writing the packet should be quick given it just buffers the data (no actual IO).
    //
    // Block until completion instead of spawning to provide backpressure to the clients.
    // NOTE: `block_on` could be removed once/if `write_packet` becomes sync.
    futures::executor::block_on(async move {
        if let Err(err) = conn.write_packet(&packet, addr).await {
            log::error!("Failed to write packet: {} (addr={})", err, addr);
        }
    });
}

#[async_trait]
impl UDPMux for UDPMuxNewAddr {
    async fn close(&self) -> Result<(), Error> {
        if self.is_closed() {
            return Err(Error::ErrAlreadyClosed);
        }

        let old_conns = {
            let mut conns = self.conns.lock().unwrap();

            std::mem::take(&mut (*conns))
        };

        // NOTE: We don't wait for these closure to complete
        for (_, conn) in old_conns {
            conn.close();
        }

        {
            let mut address_map = self.address_map.write();

            // NOTE: This is important, we need to drop all instances of `UDPMuxConn` to
            // avoid a retain cycle due to the use of [`std::sync::Arc`] on both sides.
            let _ = std::mem::take(&mut (*address_map));
        }

        {
            let mut new_addrs = self.new_addrs.write();

            // NOTE: This is important, we need to drop all instances of `UDPMuxConn` to
            // avoid a retain cycle due to the use of [`std::sync::Arc`] on both sides.
            let _ = std::mem::take(&mut (*new_addrs));
        }

        Ok(())
    }

    async fn get_conn(self: Arc<Self>, ufrag: &str) -> Result<Arc<dyn Conn + Send + Sync>, Error> {
        if self.is_closed() {
            return Err(Error::ErrUseClosedNetworkConn);
        }

        {
            let mut conns = self.conns.lock().unwrap();
            if let Some(conn) = conns.get(ufrag) {
                // UDPMuxConn uses `Arc` internally so it's cheap to clone, but because
                // we implement `Conn` we need to further wrap it in an `Arc` here.
                return Ok(Arc::new(conn.clone()) as Arc<dyn Conn + Send + Sync>);
            }

            let muxed_conn = self.create_muxed_conn(ufrag)?;
            let mut close_rx = muxed_conn.close_rx();
            let cloned_self = Arc::clone(&self);
            let cloned_ufrag = ufrag.to_string();
            tokio::spawn(async move {
                let _ = close_rx.changed().await;

                // Arc needed
                cloned_self.remove_conn_by_ufrag(&cloned_ufrag).await;
            });

            conns.insert(ufrag.into(), muxed_conn.clone());

            Ok(Arc::new(muxed_conn) as Arc<dyn Conn + Send + Sync>)
        }
    }

    async fn remove_conn_by_ufrag(&self, ufrag: &str) {
        // Pion's ice implementation has both `RemoveConnByFrag` and `RemoveConn`, but since `conns`
        // is keyed on `ufrag` their implementation is equivalent.

        let removed_conn = {
            let mut conns = self.conns.lock().unwrap();
            conns.remove(ufrag)
        };

        if let Some(conn) = removed_conn {
            let mut address_map = self.address_map.write();

            for address in conn.get_addresses() {
                address_map.remove(&address);
            }
        }
    }
}

#[async_trait]
impl UDPMuxWriter for UDPMuxNewAddr {
    async fn register_conn_for_address(&self, conn: &UDPMuxConn, addr: SocketAddr) {
        if self.is_closed() {
            return;
        }

        let key = conn.key();
        {
            let mut addresses = self.address_map.write();

            addresses
                .entry(addr)
                .and_modify(|e| {
                    if e.key() != key {
                        e.remove_address(&addr);
                        *e = conn.clone();
                    }
                })
                .or_insert_with(|| conn.clone());
        }

        // remove addr from new_addrs once conn is established
        {
            let mut new_addrs = self.new_addrs.write();
            new_addrs.remove(&addr);
        }

        log::debug!("Registered {} for {}", addr, key);
    }

    async fn send_to(&self, buf: &[u8], target: &SocketAddr) -> Result<usize, Error> {
        self.udp_sock
            .send_to(buf, *target)
            .await
            .map_err(Into::into)
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
