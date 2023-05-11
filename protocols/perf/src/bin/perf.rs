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

use std::{net::Ipv4Addr, str::FromStr};

use anyhow::{bail, Result};
use async_trait::async_trait;
use clap::Parser;
use colored::*;
use futures::{future::Either, StreamExt};
use instant::{Duration, Instant};
use libp2p_core::{
    multiaddr::Protocol, muxing::StreamMuxerBox, transport::OrTransport, upgrade, Multiaddr,
    Transport as _,
};
use libp2p_dns::TokioDnsConfig;
use libp2p_identity::PeerId;
use libp2p_perf::RunParams;
use libp2p_swarm::{Swarm, SwarmBuilder, SwarmEvent};
use log::{error, info};
use serde::{Deserialize, Serialize};

mod schema {
    use serde::{Deserialize, Serialize};
    schemafy::schemafy!(
        root: BenchmarkResults
            "src/benchmark-data-schema.json"
    );
}

#[derive(Debug, Parser)]
#[clap(name = "libp2p perf client")]
struct Opts {
    #[arg(long)]
    server_ip_address: Option<Ipv4Addr>,
    #[arg(long)]
    transport: Option<Transport>,
    #[arg(long)]
    upload_bytes: Option<usize>,
    #[arg(long)]
    download_bytes: Option<usize>,

    /// Run in server mode.
    #[clap(long)]
    run_server: bool,
    /// Fixed value to generate deterministic peer id.
    #[clap(long)]
    secret_key_seed: Option<u8>,
}

/// Supported transports by rust-libp2p.
#[derive(Clone, Debug)]
pub enum Transport {
    Tcp,
    QuicV1,
}

impl FromStr for Transport {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "tcp" => Self::Tcp,
            "quic-v1" => Self::QuicV1,
            other => bail!("unknown transport {other}"),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let opts = Opts::parse();
    match opts {
        Opts {
            server_ip_address: None,
            transport: None,
            upload_bytes: None,
            download_bytes: None,
            run_server: true,
            secret_key_seed: Some(secret_key_seed),
        } => server(secret_key_seed).await?,
        Opts {
            server_ip_address: Some(server_ip_address),
            transport: Some(transport),
            upload_bytes,
            download_bytes,
            run_server: false,
            secret_key_seed: None,
        } => {
            client(server_ip_address, transport, upload_bytes, download_bytes).await?;
        }
        _ => panic!("invalid command line arguments: {opts:?}"),
    };

    Ok(())
}

async fn server(secret_key_seed: u8) -> Result<()> {
    // Create a random PeerId
    let local_key = generate_ed25519(secret_key_seed);
    let local_peer_id = PeerId::from(local_key.public());
    println!("Local peer id: {local_peer_id}");

    let transport = {
        let tcp = libp2p_tcp::tokio::Transport::new(libp2p_tcp::Config::default().nodelay(true))
            .upgrade(upgrade::Version::V1Lazy)
            .authenticate(
                libp2p_noise::NoiseAuthenticated::xx(&local_key)
                    .expect("Signing libp2p-noise static DH keypair failed."),
            )
            .multiplex(libp2p_yamux::YamuxConfig::default());

        let quic = {
            let mut config = libp2p_quic::Config::new(&local_key);
            config.support_draft_29 = true;
            libp2p_quic::tokio::Transport::new(config)
        };

        let dns = TokioDnsConfig::system(OrTransport::new(quic, tcp)).unwrap();

        dns.map(|either_output, _| match either_output {
            Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .boxed()
    };

    let mut swarm = SwarmBuilder::with_tokio_executor(
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

    tokio::spawn(async move {
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
                SwarmEvent::Behaviour(()) => {
                    info!("Finished run",)
                }
                e => panic!("{e:?}"),
            }
        }
    })
    .await
    .unwrap();

    Ok(())
}

async fn client(
    server_ip_address: Ipv4Addr,
    transport: Transport,
    upload_bytes: Option<usize>,
    download_bytes: Option<usize>,
) -> Result<()> {
    let benchmarks: Vec<Box<dyn Benchmark>> = if upload_bytes.is_some() {
        vec![Box::new(Custom {
            upload_bytes: upload_bytes.unwrap(),
            download_bytes: download_bytes.unwrap(),
        })]
    } else {
        vec![
            Box::new(Latency {}),
            // Box::new(Throughput {}),
            Box::new(RequestsPerSecond {}),
            Box::new(ConnectionsPerSecond {}),
        ]
    };

    let address = match transport {
        Transport::Tcp => Multiaddr::empty()
            .with(Protocol::Ip4(server_ip_address))
            .with(Protocol::Tcp(4001)),
        Transport::QuicV1 => Multiaddr::empty()
            .with(Protocol::Ip4(server_ip_address))
            .with(Protocol::Udp(4001))
            .with(Protocol::QuicV1),
    };

    let results = tokio::spawn(async move {
        let mut results = vec![];

        for benchmark in benchmarks {
            info!(
                "{}",
                format!("Start benchmark: {}", benchmark.name()).underline(),
            );

            let result = benchmark.run(address.clone()).await?;
            if let Some(result) = result {
                results.push(result);
            }
        }

        anyhow::Ok(results)
    })
    .await??;

    if !results.is_empty() {
        println!(
            "{}",
            serde_json::to_string(&schema::Benchmarks {
                benchmarks: results,
                schema: None,
            })
            .unwrap()
        );
    }

    Ok(())
}

#[async_trait]
trait Benchmark: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    async fn run(&self, server_address: Multiaddr) -> Result<Option<schema::Benchmark>>;
}

struct Custom {
    upload_bytes: usize,
    download_bytes: usize,
}

#[async_trait]
impl Benchmark for Custom {
    fn name(&self) -> &'static str {
        "custom"
    }

    async fn run(&self, server_address: Multiaddr) -> Result<Option<schema::Benchmark>> {
        let mut latencies = Vec::new();

        let mut swarm = swarm().await;

        let server_peer_id = connect(&mut swarm, server_address.clone()).await?;

        let start = Instant::now();

        swarm.behaviour_mut().perf(
            server_peer_id,
            RunParams {
                to_send: self.upload_bytes,
                to_receive: self.download_bytes,
            },
        )?;

        loop {
            match swarm.next().await.unwrap() {
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    info!("Established connection to {:?} via {:?}", peer_id, endpoint);
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error } => {
                    info!("Outgoing connection error to {:?}: {:?}", peer_id, error);
                }
                SwarmEvent::Behaviour(libp2p_perf::client::Event {
                    id: _,
                    result: Ok(()),
                }) => break,
                e => panic!("{e:?}"),
            };
        }

        latencies.push(start.elapsed().as_secs_f64());

        info!(
            "Finished: Sent perf request on single connection uploading {} and download {} bytes each",
            self.upload_bytes, self.download_bytes
        );

        println!(
            "{}",
            serde_json::to_string(&CustomResult { latencies }).unwrap()
        );

        Ok(None)
    }
}

#[derive(Serialize, Deserialize)]
struct CustomResult {
    latencies: Vec<f64>,
}

struct Latency {}

#[async_trait]
impl Benchmark for Latency {
    fn name(&self) -> &'static str {
        "round-trip-time latency"
    }

    async fn run(&self, server_address: Multiaddr) -> Result<Option<schema::Benchmark>> {
        let mut swarm = swarm().await;

        let server_peer_id = connect(&mut swarm, server_address.clone()).await?;

        let mut rounds = 0;
        let start = Instant::now();
        let mut latencies = Vec::new();

        loop {
            if start.elapsed() > Duration::from_secs(30) {
                break;
            }

            let start = Instant::now();

            swarm.behaviour_mut().perf(
                server_peer_id,
                RunParams {
                    to_send: 1,
                    to_receive: 1,
                },
            )?;

            loop {
                match swarm.next().await.unwrap() {
                    SwarmEvent::ConnectionEstablished {
                        peer_id, endpoint, ..
                    } => {
                        info!("Established connection to {:?} via {:?}", peer_id, endpoint);
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error } => {
                        info!("Outgoing connection error to {:?}: {:?}", peer_id, error);
                    }
                    SwarmEvent::Behaviour(libp2p_perf::client::Event {
                        id: _,
                        result: Ok(()),
                    }) => break,
                    e => panic!("{e:?}"),
                };
            }
            latencies.push(start.elapsed().as_secs_f64());
            rounds += 1;
        }

        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

        info!(
            "Finished: {rounds} pings in {:.4}s",
            start.elapsed().as_secs_f64()
        );
        info!("- {:.4} s median", percentile(&latencies, 0.50),);
        info!("- {:.4} s 95th percentile\n", percentile(&latencies, 0.95),);
        Ok(None)
    }
}

fn percentile<V: PartialOrd + Copy>(values: &[V], percentile: f64) -> V {
    let n: usize = (values.len() as f64 * percentile).ceil() as usize - 1;
    values[n]
}

// // TODO: Bring back?
// struct Throughput {}
//
// #[async_trait]
// impl Benchmark for Throughput {
//     fn name(&self) -> &'static str {
//         "single connection single channel throughput"
//     }
//
//     async fn run(&self, server_address: Multiaddr) -> Result<Option<schema::Benchmark>> {
//         let mut swarm = swarm().await;
//
//         let server_peer_id = connect(&mut swarm, server_address.clone()).await?;
//
//         let params =
//             RunParams {
//                 to_send: 10 * 1024 * 1024,
//                 to_receive: 10 * 1024 * 1024,
//             };
//
//         swarm.behaviour_mut().perf(
//             server_peer_id,
//             params,
//         )?;
//
//         let stats = loop {
//             match swarm.next().await.unwrap() {
//                 SwarmEvent::ConnectionEstablished {
//                     peer_id, endpoint, ..
//                 } => {
//                     info!("Established connection to {:?} via {:?}", peer_id, endpoint);
//                 }
//                 SwarmEvent::OutgoingConnectionError { peer_id, error } => {
//                     info!("Outgoing connection error to {:?}: {:?}", peer_id, error);
//                 }
//                 SwarmEvent::Behaviour(libp2p_perf::client::Event { id: _, result }) => {
//                     break result?
//                 }
//                 e => panic!("{e:?}"),
//             }
//         };
//
//         let sent_mebibytes = params.to_send as f64 / 1024.0 / 1024.0;
//         let sent_time = (stats.timers.write_done - stats.timers.write_start).as_secs_f64();
//         let sent_bandwidth_mebibit_second = (sent_mebibytes * 8.0) / sent_time;
//
//         let received_mebibytes = params.to_receive as f64 / 1024.0 / 1024.0;
//         let receive_time = (stats.timers.read_done - stats.timers.write_done).as_secs_f64();
//         let receive_bandwidth_mebibit_second = (received_mebibytes * 8.0) / receive_time;
//
//         info!(
//             "Finished: sent {sent_mebibytes:.2} MiB \
//              and received {received_mebibytes:.2} MiB in {time:.2} s",
//         );
//         info!("- {sent_bandwidth_mebibit_second:.2} MiBit/s up");
//         info!("- {receive_bandwidth_mebibit_second:.2} MiBit/s down\n");
//
//         Ok(Some(schema::Benchmark {
//             name: "Single Connection throughput – Upload".to_string(),
//             unit: "bits/s".to_string(),
//             comparisons: vec![],
//             results: vec![schema::Result {
//                 implementation: "rust-libp2p".to_string(),
//                 transport_stack: "TODO".to_string(),
//                 version: "TODO".to_string(),
//                 result: stats.params.to_send as f64 / sent_time,
//             }],
//         }))
//     }
// }

struct RequestsPerSecond {}

#[async_trait]
impl Benchmark for RequestsPerSecond {
    fn name(&self) -> &'static str {
        "single connection parallel requests per second"
    }

    async fn run(&self, server_address: Multiaddr) -> Result<Option<schema::Benchmark>> {
        let mut swarm = swarm().await;

        let server_peer_id = connect(&mut swarm, server_address.clone()).await?;

        let num = 1_000;
        let to_send = 1;
        let to_receive = 1;

        for _ in 0..num {
            swarm.behaviour_mut().perf(
                server_peer_id,
                RunParams {
                    to_send,
                    to_receive,
                },
            )?;
        }

        let mut finished = 0;
        let start = Instant::now();

        loop {
            match swarm.next().await.unwrap() {
                SwarmEvent::ConnectionEstablished {
                    peer_id, endpoint, ..
                } => {
                    info!("Established connection to {:?} via {:?}", peer_id, endpoint);
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error } => {
                    info!("Outgoing connection error to {:?}: {:?}", peer_id, error);
                }
                SwarmEvent::Behaviour(libp2p_perf::client::Event {
                    id: _,
                    result: Ok(_),
                }) => {
                    finished += 1;

                    if finished == num {
                        break;
                    }
                }
                e => panic!("{e:?}"),
            }
        }

        let duration = start.elapsed().as_secs_f64();
        let requests_per_second = num as f64 / duration;

        info!(
            "Finished: sent {num} {to_send} bytes requests with {to_receive} bytes response each within {duration:.2} s",
        );
        info!("- {requests_per_second:.2} req/s\n");

        Ok(None)
    }
}

struct ConnectionsPerSecond {}

#[async_trait]
impl Benchmark for ConnectionsPerSecond {
    fn name(&self) -> &'static str {
        "sequential connections with single request per second"
    }

    async fn run(&self, server_address: Multiaddr) -> Result<Option<schema::Benchmark>> {
        let mut rounds = 0;
        let to_send = 1;
        let to_receive = 1;
        let start = Instant::now();

        let mut latency_connection_establishment = Vec::new();
        let mut latency_connection_establishment_plus_request = Vec::new();

        loop {
            if start.elapsed() > Duration::from_secs(30) {
                break;
            }

            let mut swarm = swarm().await;

            let start = Instant::now();

            let server_peer_id = connect(&mut swarm, server_address.clone()).await?;

            latency_connection_establishment.push(start.elapsed().as_secs_f64());

            swarm.behaviour_mut().perf(
                server_peer_id,
                RunParams {
                    to_send,
                    to_receive,
                },
            )?;

            match swarm.next().await.unwrap() {
                SwarmEvent::Behaviour(libp2p_perf::client::Event {
                    id: _,
                    result: Ok(_),
                }) => {}
                e => panic!("{e:?}"),
            };

            latency_connection_establishment_plus_request.push(start.elapsed().as_secs_f64());
            rounds += 1;
        }

        let duration = start.elapsed().as_secs_f64();

        latency_connection_establishment.sort_by(|a, b| a.partial_cmp(b).unwrap());
        latency_connection_establishment_plus_request.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let connection_establishment_95th = percentile(&latency_connection_establishment, 0.95);
        let connection_establishment_plus_request_95th =
            percentile(&latency_connection_establishment_plus_request, 0.95);

        info!(
            "Finished: established {rounds} connections with one {to_send} bytes request and one {to_receive} bytes response within {duration:.2} s",
        );
        info!("- {connection_establishment_95th:.4} s 95th percentile connection establishment");
        info!("- {connection_establishment_plus_request_95th:.4} s 95th percentile connection establishment + one request");

        Ok(Some(schema::Benchmark {
            name: "Single Connection 1 byte round trip latency 95th percentile".to_string(),
            unit: "s".to_string(),
            comparisons: vec![],
            results: vec![schema::Result {
                implementation: "rust-libp2p".to_string(),
                transport_stack: "TODO".to_string(),
                version: "TODO".to_string(),
                result: connection_establishment_plus_request_95th,
            }],
        }))
    }
}

async fn swarm() -> Swarm<libp2p_perf::client::Behaviour> {
    // Create a random PeerId
    let local_key = libp2p_identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let transport = {
        let tcp = libp2p_tcp::tokio::Transport::new(libp2p_tcp::Config::default().nodelay(true))
            .upgrade(upgrade::Version::V1Lazy)
            .authenticate(
                libp2p_noise::NoiseAuthenticated::xx(&local_key)
                    .expect("Signing libp2p-noise static DH keypair failed."),
            )
            .multiplex(libp2p_yamux::YamuxConfig::default());

        let quic = {
            let mut config = libp2p_quic::Config::new(&local_key);
            config.support_draft_29 = true;
            libp2p_quic::tokio::Transport::new(config)
        };

        let dns = TokioDnsConfig::system(OrTransport::new(quic, tcp)).unwrap();

        dns.map(|either_output, _| match either_output {
            Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .boxed()
    };

    SwarmBuilder::with_tokio_executor(
        transport,
        libp2p_perf::client::Behaviour::default(),
        local_peer_id,
    )
    .substream_upgrade_protocol_override(upgrade::Version::V1Lazy)
    .build()
}

async fn connect(
    swarm: &mut Swarm<libp2p_perf::client::Behaviour>,
    server_address: Multiaddr,
) -> Result<PeerId> {
    swarm.dial(server_address.clone()).unwrap();

    let server_peer_id = loop {
        match swarm.next().await.unwrap() {
            SwarmEvent::ConnectionEstablished { peer_id, .. } => break peer_id,
            SwarmEvent::OutgoingConnectionError { peer_id, error } => {
                bail!("Outgoing connection error to {:?}: {:?}", peer_id, error);
            }
            e => panic!("{e:?}"),
        }
    };

    Ok(server_peer_id)
}

fn generate_ed25519(secret_key_seed: u8) -> libp2p_identity::Keypair {
    let mut bytes = [0u8; 32];
    bytes[0] = secret_key_seed;

    libp2p_identity::Keypair::ed25519_from_bytes(bytes).expect("only errors on wrong length")
}
