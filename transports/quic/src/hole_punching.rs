use crate::{Connecting, Connection, Error};

use libp2p_identity::PeerId;

use futures::{
    channel::oneshot,
    future::{Fuse, FusedFuture},
    prelude::*,
};
use futures_timer::Delay;

use rand::{distributions, Rng};

use std::{
    collections::HashMap,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

pub(crate) type HolePunchMap =
    Arc<Mutex<HashMap<(SocketAddr, PeerId), oneshot::Sender<(PeerId, Connection)>>>>;

pub(crate) struct MaybeHolePunchedConnection {
    hole_punch_map: HolePunchMap,
    addr: SocketAddr,
    upgrade: Fuse<Connecting>,
}

impl MaybeHolePunchedConnection {
    pub(crate) fn new(hole_punch_map: HolePunchMap, addr: SocketAddr, upgrade: Connecting) -> Self {
        Self {
            hole_punch_map,
            addr,
            upgrade: upgrade.fuse(),
        }
    }
}

impl Future for MaybeHolePunchedConnection {
    type Output = Result<(PeerId, Connection), Error>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let (peer_id, connection) = futures::ready!(self.upgrade.poll_unpin(cx))?;
        let addr = self.addr;
        let mut hole_punch_map = self.hole_punch_map.lock().unwrap();
        if let Some(sender) = hole_punch_map.remove(&(addr, peer_id)) {
            if let Err(connection) = sender.send((peer_id, connection)) {
                Poll::Ready(Ok(connection))
            } else {
                Poll::Pending
            }
        } else {
            Poll::Ready(Ok((peer_id, connection)))
        }
    }
}

impl FusedFuture for MaybeHolePunchedConnection {
    fn is_terminated(&self) -> bool {
        self.upgrade.is_terminated()
    }
}

pub(crate) struct HolePuncher {
    socket: socket2::Socket,
    timeout: Delay,
    interval_timeout: Delay,
}

impl HolePuncher {
    pub(crate) fn new(
        local_addr: SocketAddr,
        remote_addr: SocketAddr,
        timeout: Duration,
    ) -> io::Result<Self> {
        let domain = socket2::Domain::for_address(remote_addr);
        let socket = socket2::Socket::new(domain, socket2::Type::DGRAM, None)?;
        socket.set_reuse_port(true)?;
        socket.bind(&local_addr.into())?;
        socket.connect(&remote_addr.into())?;

        Ok(Self {
            socket,
            timeout: Delay::new(timeout),
            interval_timeout: Delay::new(Duration::from_secs(0)),
        })
    }
}

/// Never finishes successfully, only with an Err (timeout)
impl Future for HolePuncher {
    type Output = Error;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.timeout.poll_unpin(cx) {
            Poll::Ready(_) => return Poll::Ready(Error::HandshakeTimedOut),
            Poll::Pending => {}
        }

        futures::ready!(self.interval_timeout.poll_unpin(cx));

        let contents: Vec<u8> = rand::thread_rng()
            .sample_iter(distributions::Standard)
            .take(64)
            .collect();

        if let Err(e) = self.socket.send(&contents) {
            if !matches!(e.kind(), io::ErrorKind::WouldBlock) {
                return Poll::Ready(Error::Io(e));
            }
        }

        self.interval_timeout.reset(Duration::from_millis(
            rand::thread_rng().gen_range(10..=200),
        ));

        Poll::Pending
    }
}
