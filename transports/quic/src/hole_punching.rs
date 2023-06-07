use std::{
    collections::{hash_map::Entry, HashMap},
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::channel::oneshot;
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

type HolePunchKey = (SocketAddr, PeerId);
type HolePunchValue = oneshot::Sender<(PeerId, Connection)>;

#[derive(Clone, Debug, Default)]
pub(crate) struct HolePunchMap(Arc<Mutex<HashMap<HolePunchKey, HolePunchValue>>>);

impl HolePunchMap {
    pub(crate) fn remove(&self, key: &HolePunchKey) -> Option<HolePunchValue> {
        self.0.lock().unwrap().remove(key)
    }

    pub(crate) fn try_insert(&self, key: HolePunchKey, value: HolePunchValue) -> bool {
        match self.0.lock().unwrap().entry(key) {
            Entry::Vacant(entry) => {
                entry.insert(value);
                true
            }
            Entry::Occupied(_) => false,
        }
    }
}

/// An upgrading inbound QUIC connection that is either
/// - a normal inbound connection or
/// - an inbound connection corresponding to an in-progress outbound hole punching connection.
pub(crate) async fn maybe_hole_punched_connection(
    hole_punch_attempts: HolePunchMap,
    remote_addr: SocketAddr,
    upgrade: Connecting,
) -> Result<(PeerId, Connection), Error> {
    let (peer_id, connection) = upgrade.await?;

    if let Some(sender) = hole_punch_attempts.remove(&(remote_addr, peer_id)) {
        if let Err((peer_id, connection)) = sender.send((peer_id, connection)) {
            Ok((peer_id, connection))
        } else {
            Err(Error::SuccessfulHolePunchRedirectingConnToDialer)
        }
    } else {
        Ok((peer_id, connection))
    }
}

pub(crate) async fn hole_puncher(
    endpoint_channel: endpoint::Channel,
    remote_addr: SocketAddr,
    timeout_duration: Duration,
) -> Error {
    timeout(timeout_duration, punch_holes(endpoint_channel, remote_addr))
        .await
        .unwrap_or(Error::HandshakeTimedOut)
}

async fn punch_holes(mut endpoint_channel: endpoint::Channel, remote_addr: SocketAddr) -> Error {
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
