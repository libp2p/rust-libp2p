use std::{net::SocketAddr, time::Duration};

use rand::{distributions, Rng};

use crate::{
    endpoint::{self, ToEndpoint},
    Error, Provider,
};

pub(crate) async fn hole_puncher<P: Provider>(
    endpoint_channel: endpoint::Channel,
    remote_addr: SocketAddr,
    timeout_duration: Duration,
) -> Error {
    P::timeout(
        timeout_duration,
        punch_holes::<P>(endpoint_channel, remote_addr),
    )
    .await
    .unwrap_or(Error::HandshakeTimedOut)
}

async fn punch_holes<P: Provider>(
    mut endpoint_channel: endpoint::Channel,
    remote_addr: SocketAddr,
) -> Error {
    loop {
        let sleep_duration = Duration::from_millis(rand::thread_rng().gen_range(10..=200));
        P::sleep(sleep_duration).await;

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
