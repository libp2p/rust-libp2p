use std::{
    collections::HashMap,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use futures::{
    channel::oneshot,
    future::{Fuse, FusedFuture},
    prelude::*,
};
use futures_timer::Delay;
use libp2p_identity::PeerId;
use rand::{distributions, Rng};

use crate::{
    endpoint::{self, ToEndpoint},
    Connecting, Connection, Error,
};

pub(crate) type HolePunchMap =
    Arc<Mutex<HashMap<(SocketAddr, PeerId), oneshot::Sender<(PeerId, Connection)>>>>;

/// An upgrading inbound QUIC connection that is either
/// - a normal inbound connection or
/// - an inbound connection corresponding to an in-progress outbound hole punching connection.
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
    endpoint_channel: endpoint::Channel,
    remote_addr: SocketAddr,
    timeout: Delay,
    interval: Delay,
    message: Option<ToEndpoint>,
}

impl HolePuncher {
    pub(crate) fn new(
        endpoint_channel: endpoint::Channel,
        remote_addr: SocketAddr,
        timeout: Duration,
    ) -> Self {
        Self {
            endpoint_channel,
            remote_addr,
            timeout: Delay::new(timeout),
            interval: Delay::new(Duration::from_secs(0)),
            message: None,
        }
    }
}

/// Never finishes successfully, only with an Err (timeout)
impl Future for HolePuncher {
    type Output = Error;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.timeout.poll_unpin(cx) {
                Poll::Ready(_) => return Poll::Ready(Error::HandshakeTimedOut),
                Poll::Pending => {}
            }

            let message = match self.message.take() {
                Some(m) => m,
                None => {
                    futures::ready!(self.interval.poll_unpin(cx));
                    self.interval.reset(Duration::from_millis(
                        rand::thread_rng().gen_range(10..=200),
                    ));
                    ToEndpoint::SendUdpPacket(quinn_proto::Transmit {
                        destination: self.remote_addr,
                        ecn: None,
                        contents: rand::thread_rng()
                            .sample_iter(distributions::Standard)
                            .take(64)
                            .collect(),
                        segment_size: None,
                        src_ip: None,
                    })
                }
            };

            match self.endpoint_channel.try_send(message, cx) {
                Ok(Ok(())) => {
                    // Message sent. Continue to register waker on `self.interval`.
                    continue;
                }
                Ok(Err(m)) => {
                    // Endpoint is busy. Try again later.
                    self.message = Some(m);
                }
                Err(endpoint::Disconnected {}) => {
                    return Poll::Ready(Error::EndpointDriverCrashed);
                }
            }

            return Poll::Pending;
        }
    }
}
