use std::{
    collections::HashMap,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};

use futures::{channel::oneshot, prelude::*};
use libp2p_identity::PeerId;
use rand::{distributions, Rng};

#[cfg(feature = "async-std")]
use async_std::{future::timeout, task::sleep};
#[cfg(feature = "tokio")]
use tokio::time::{sleep, timeout};

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
    upgrade: Connecting,
}

impl MaybeHolePunchedConnection {
    pub(crate) fn new(hole_punch_map: HolePunchMap, addr: SocketAddr, upgrade: Connecting) -> Self {
        Self {
            hole_punch_map,
            addr,
            upgrade,
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
                Poll::Ready(Err(Error::HandshakeTimedOut))
            }
        } else {
            Poll::Ready(Ok((peer_id, connection)))
        }
    }
}

async fn punch_holes(
    mut endpoint_channel: endpoint::Channel,
    remote_addr: SocketAddr,
) -> Error {
    loop {
        let sleep_duration = Duration::from_millis(rand::thread_rng().gen_range(10..=200));
        sleep(sleep_duration).await;

        let random_udp_packet = ToEndpoint::SendUdpPacket(quinn_proto::Transmit {
            destination: remote_addr,
            ecn: None,
            contents: rand::thread_rng()
                .sample_iter(distributions::Standard)
                .take(64)
                .collect(),
            segment_size: None,
            src_ip: None,
        });

        if endpoint_channel.send(random_udp_packet).await.is_err() {
            return Error::EndpointDriverCrashed;
        }
    }
}

pub(crate) async fn hole_puncher(
    endpoint_channel: endpoint::Channel,
    remote_addr: SocketAddr,
    timeout_duration: Duration,
) -> Error {
    timeout(
        timeout_duration,
        punch_holes(endpoint_channel, remote_addr),
    )
    .await
    .unwrap_or(Error::HandshakeTimedOut)
}
