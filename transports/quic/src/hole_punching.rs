use std::{net::SocketAddr, time::Duration};

use rand::{distributions, Rng};

#[cfg(feature = "async-std")]
use async_std::{future::timeout, task::sleep};
#[cfg(feature = "tokio")]
use tokio::time::{sleep, timeout};

use crate::{
    endpoint::{self, ToEndpoint},
    Error,
};

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
