// Copyright 2023 Protocol Labs.
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

use clap::Parser;
use futures::{future::Either, StreamExt};
use libp2p_core::{muxing::StreamMuxerBox, transport::OrTransport, upgrade, Transport};
use libp2p_dns::DnsConfig;
use libp2p_identity::PeerId;
use libp2p_swarm::{SwarmBuilder, SwarmEvent};
use log::{error, info};

#[derive(Debug, Parser)]
#[clap(name = "libp2p perf server")]
struct Opts {}

#[async_std::main]
async fn main() {
    env_logger::init();

    let _opts = Opts::parse();

    // Create a random PeerId
    let local_key = libp2p_identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {local_peer_id}");

    let transport = {
        let tcp =
            libp2p_tcp::async_io::Transport::new(libp2p_tcp::Config::default().port_reuse(true))
                .upgrade(upgrade::Version::V1Lazy)
                .authenticate(
                    libp2p_noise::Config::new(&local_key)
                        .expect("Signing libp2p-noise static DH keypair failed."),
                )
                .multiplex(libp2p_yamux::Config::default());

        let quic = {
            let mut config = libp2p_quic::Config::new(&local_key);
            config.support_draft_29 = true;
            libp2p_quic::async_std::Transport::new(config)
        };

        let dns = DnsConfig::system(OrTransport::new(quic, tcp))
            .await
            .unwrap();

        dns.map(|either_output, _| match either_output {
            Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .boxed()
    };

    let mut swarm = SwarmBuilder::with_async_std_executor(
        transport,
        libp2p_perf::server::Behaviour::default(),
        local_peer_id,
    )
    .substream_upgrade_protocol_override(upgrade::Version::V1Lazy)
    .build();

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/4001".parse().unwrap())
        .unwrap();

    swarm
        .listen_on("/ip4/0.0.0.0/udp/4001/quic-v1".parse().unwrap())
        .unwrap();

    loop {
        match swarm.next().await.unwrap() {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {address}");
            }
            SwarmEvent::IncomingConnection { .. } => {}
            e @ SwarmEvent::IncomingConnectionError { .. } => {
                error!("{e:?}");
            }
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                info!("Established connection to {:?} via {:?}", peer_id, endpoint);
            }
            SwarmEvent::ConnectionClosed { .. } => {}
            SwarmEvent::Behaviour(libp2p_perf::server::Event {
                remote_peer_id,
                stats,
            }) => {
                let received_mebibytes = stats.params.received as f64 / 1024.0 / 1024.0;
                let receive_time = (stats.timers.read_done - stats.timers.read_start).as_secs_f64();
                let receive_bandwidth_mebibit_second = (received_mebibytes * 8.0) / receive_time;

                let sent_mebibytes = stats.params.sent as f64 / 1024.0 / 1024.0;
                let sent_time = (stats.timers.write_done - stats.timers.read_done).as_secs_f64();
                let sent_bandwidth_mebibit_second = (sent_mebibytes * 8.0) / sent_time;

                info!(
                        "Finished run with {}: Received {:.2} MiB in {:.2} s with {:.2} MiBit/s and sent {:.2} MiB in {:.2} s with {:.2} MiBit/s",
                        remote_peer_id,
                        received_mebibytes,
                        receive_time,
                        receive_bandwidth_mebibit_second,
                        sent_mebibytes,
                        sent_time,
                        sent_bandwidth_mebibit_second,
                    )
            }
            e => panic!("{e:?}"),
        }
    }
}
