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

use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    io::ErrorKind,
    net::SocketAddr,
    sync::Arc,
};

use futures::channel::mpsc;
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use webrtc_ice::udp_mux::UDPMux;
use webrtc_util::{sync::RwLock, Conn, Error};

use tokio_crate as tokio;
use tokio_crate::sync::{watch, Mutex};

mod socket_addr_ext;

mod udp_mux_conn;
use udp_mux_conn::{UDPMuxConn, UDPMuxConnParams};

use async_trait::async_trait;

use stun::{
    attributes::ATTR_USERNAME,
    message::{is_message as is_stun_message, Message as STUNMessage},
};

const RECEIVE_MTU: usize = 8192;

/// Normalize a target socket addr for sending over a given local socket addr. This is useful when
/// a dual stack socket is used, in which case an IPv4 target needs to be mapped to an IPv6
/// address.
fn normalize_socket_addr(target: &SocketAddr, socket_addr: &SocketAddr) -> SocketAddr {
    match (target, socket_addr) {
        (SocketAddr::V4(target_ipv4), SocketAddr::V6(_)) => {
            let ipv6_mapped = target_ipv4.ip().to_ipv6_mapped();

            SocketAddr::new(std::net::IpAddr::V6(ipv6_mapped), target_ipv4.port())
        },
        // This will fail later if target is IPv6 and socket is IPv4, we ignore it here
        (_, _) => *target,
    }
}

pub struct UDPMuxParams {
    conn: Box<dyn Conn + Send + Sync>,
}

impl UDPMuxParams {
    pub fn new<C>(conn: C) -> Self
    where
        C: Conn + Send + Sync + 'static,
    {
        Self {
            conn: Box::new(conn),
        }
    }
}

/// This is a copy of `UDPMuxDefault` with the exception of ability to report new addresses via
/// `new_addr_tx`.
pub struct UDPMuxNewAddr {
    /// The params this instance is configured with.
    /// Contains the underlying UDP socket in use
    params: UDPMuxParams,

    /// Maps from ufrag to the underlying connection.
    conns: Mutex<HashMap<String, UDPMuxConn>>,

    /// Maps from ip address to the underlying connection.
    address_map: RwLock<HashMap<SocketAddr, UDPMuxConn>>,

    // Close sender
    closed_watch_tx: Mutex<Option<watch::Sender<()>>>,

    /// Close reciever
    closed_watch_rx: watch::Receiver<()>,

    /// Set of the new IP addresses reported via `new_addr_tx` to avoid sending the same IP
    /// multiple times.
    new_addrs: RwLock<HashSet<SocketAddr>>,
}

impl UDPMuxNewAddr {
    pub fn new(params: UDPMuxParams, new_addr_tx: mpsc::Sender<Multiaddr>) -> Arc<Self> {
        let (closed_watch_tx, closed_watch_rx) = watch::channel(());

        let mux = Arc::new(Self {
            params,
            conns: Mutex::default(),
            address_map: RwLock::default(),
            closed_watch_tx: Mutex::new(Some(closed_watch_tx)),
            closed_watch_rx: closed_watch_rx.clone(),
            new_addrs: RwLock::default(),
        });

        let cloned_mux = Arc::clone(&mux);
        cloned_mux.start_conn_worker(closed_watch_rx, new_addr_tx);

        mux
    }

    pub async fn is_closed(&self) -> bool {
        self.closed_watch_tx.lock().await.is_none()
    }

    async fn send_to(&self, buf: &[u8], target: &SocketAddr) -> Result<usize, Error> {
        self.params
            .conn
            .send_to(buf, *target)
            .await
            .map_err(Into::into)
    }

    /// Create a muxed connection for a given ufrag.
    async fn create_muxed_conn(self: &Arc<Self>, ufrag: &str) -> Result<UDPMuxConn, Error> {
        let local_addr = self.params.conn.local_addr().await?;

        let params = UDPMuxConnParams {
            local_addr,
            key: ufrag.into(),
            udp_mux: Arc::clone(self),
        };

        Ok(UDPMuxConn::new(params))
    }

    async fn register_conn_for_address(&self, conn: &UDPMuxConn, addr: SocketAddr) {
        if self.is_closed().await {
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
                        *e = conn.clone()
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

    async fn conn_from_stun_message(&self, buffer: &[u8], addr: &SocketAddr) -> Option<UDPMuxConn> {
        match ufrag_from_stun_message(buffer, true) {
            Ok(ufrag) => {
                let conns = self.conns.lock().await;
                conns.get(&ufrag).map(Clone::clone)
            },
            Err(e) => {
                log::error!("{} (addr: {})", e, addr);
                None
            },
        }
    }

    fn start_conn_worker(
        self: Arc<Self>,
        mut closed_watch_rx: watch::Receiver<()>,
        mut new_addr_tx: mpsc::Sender<Multiaddr>,
    ) {
        tokio::spawn(async move {
            let mut buffer = [0u8; RECEIVE_MTU];

            loop {
                let loop_self = Arc::clone(&self);
                let conn = &loop_self.params.conn;

                tokio::select! {
                    res = conn.recv_from(&mut buffer) => {
                        match res {
                            Ok((len, addr)) => {
                                // Find connection based on previously having seen this source address
                                let conn = {
                                    let address_map = loop_self
                                        .address_map
                                        .read();

                                    address_map.get(&addr).map(Clone::clone)
                                };

                                let conn = match conn {
                                    // If we couldn't find the connection based on source address, see if
                                    // this is a STUN mesage and if so if we can find the connection based on ufrag.
                                    None if is_stun_message(&buffer) => {
                                        loop_self.conn_from_stun_message(&buffer, &addr).await
                                    }
                                    s @ Some(_) => s,
                                    _ => None,
                                };

                                match conn {
                                    None => {
                                        if !loop_self.new_addrs.read().contains(&addr) {
                                            match ufrag_from_stun_message(&buffer, false) {
                                                Ok(ufrag) => {
                                                    log::trace!("Notifying about new address {} from {}", &addr, ufrag);
                                                    let a = Multiaddr::empty()
                                                        .with(addr.ip().into())
                                                        .with(Protocol::Udp(addr.port()))
                                                        .with(Protocol::XWebRTC(hex_to_cow(&ufrag.replace(":", ""))));
                                                    if let Err(err) = new_addr_tx.try_send(a) {
                                                        log::error!("Failed to send new address {}: {}", &addr, err);
                                                    } else {
                                                        let mut new_addrs = loop_self.new_addrs.write();
                                                        new_addrs.insert(addr.clone());
                                                    };
                                                }
                                                Err(e) => {
                                                    log::trace!("Unknown address {} (non STUN packet: {})", &addr, e);
                                                }
                                            }
                                        }
                                    }
                                    Some(conn) => {
                                        if let Err(err) = conn.write_packet(&buffer[..len], addr).await {
                                            log::error!("Failed to write packet: {}", err);
                                        }
                                    }
                                }
                            }
                            Err(Error::Io(err)) if err.0.kind() == ErrorKind::TimedOut => continue,
                            Err(err) => {
                                log::error!("Could not read udp packet: {}", err);
                                break;
                            }
                        }
                    }
                    _ = closed_watch_rx.changed() => {
                        return;
                    }
                }
            }
        });
    }
}

#[async_trait]
impl UDPMux for UDPMuxNewAddr {
    async fn close(&self) -> Result<(), Error> {
        if self.is_closed().await {
            return Err(Error::ErrAlreadyClosed);
        }

        let mut closed_tx = self.closed_watch_tx.lock().await;

        if let Some(tx) = closed_tx.take() {
            let _ = tx.send(());
            drop(closed_tx);

            let old_conns = {
                let mut conns = self.conns.lock().await;

                std::mem::take(&mut (*conns))
            };

            // NOTE: We don't wait for these closure to complete
            for (_, conn) in old_conns.into_iter() {
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
        }

        Ok(())
    }

    async fn get_conn(self: Arc<Self>, ufrag: &str) -> Result<Arc<dyn Conn + Send + Sync>, Error> {
        if self.is_closed().await {
            return Err(Error::ErrUseClosedNetworkConn);
        }

        {
            let mut conns = self.conns.lock().await;
            if let Some(conn) = conns.get(ufrag) {
                // UDPMuxConn uses `Arc` internally so it's cheap to clone, but because
                // we implement `Conn` we need to further wrap it in an `Arc` here.
                return Ok(Arc::new(conn.clone()) as Arc<dyn Conn + Send + Sync>);
            }

            let muxed_conn = self.create_muxed_conn(ufrag).await?;
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
            let mut conns = self.conns.lock().await;
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

fn hex_to_cow<'a>(s: &str) -> Cow<'a, [u8; 32]> {
    let mut buf = [0; 32];
    hex::decode_to_slice(s, &mut buf).unwrap();
    Cow::Owned(buf)
}

/// Gets the ufrag from the given STUN message or returns an error, if failed to decode or the
/// username attribute is not present.
fn ufrag_from_stun_message(buffer: &[u8], local_ufrag: bool) -> Result<String, Error> {
    let (result, message) = {
        let mut m = STUNMessage::new();

        (m.unmarshal_binary(buffer), m)
    };

    match result {
        Err(err) => Err(Error::Other(format!(
            "failed to handle decode ICE: {}",
            err
        ))),
        Ok(_) => {
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
                        s.split(":").next()
                    } else {
                        s.split(":").last()
                    };
                    match res {
                        Some(s) => Ok(s.to_owned()),
                        None => Err(Error::Other("can't get ufrag from username".into())),
                    }
                },
            }
        },
    }
}
