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

use std::{net::SocketAddr, str::FromStr};

use anyhow::{bail, Result};
use clap::Parser;
use futures::FutureExt;
use futures::{future::Either, StreamExt};
use instant::{Duration, Instant};
use libp2p_core::{
    multiaddr::Protocol, muxing::StreamMuxerBox, transport::OrTransport, upgrade, Multiaddr,
    Transport as _,
};
use libp2p_identity::PeerId;
use libp2p_perf::{Run, RunDuration, RunParams};
use libp2p_swarm::{Config, NetworkBehaviour, Swarm, SwarmEvent};
use log::{error, info};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[clap(name = "libp2p perf client")]
struct Opts {
    #[arg(long)]
    server_address: Option<SocketAddr>,
    #[arg(long)]
    transport: Option<Transport>,
    #[arg(long)]
    upload_bytes: Option<usize>,
    #[arg(long)]
    download_bytes: Option<usize>,

    /// Run in server mode.
    #[clap(long)]
    run_server: bool,
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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let opts = Opts::parse();
    match opts {
        Opts {
            server_address: Some(server_address),
            transport: None,
            upload_bytes: None,
            download_bytes: None,
            run_server: true,
        } => server(server_address).await?,
        Opts {
            server_address: Some(server_address),
            transport: Some(transport),
            upload_bytes,
            download_bytes,
            run_server: false,
        } => {
            client(server_address, transport, upload_bytes, download_bytes).await?;
        }
        _ => panic!("invalid command line arguments: {opts:?}"),
    };

    Ok(())
}

async fn server(server_address: SocketAddr) -> Result<()> {
    let mut swarm = swarm::<libp2p_perf::server::Behaviour>().await?;

    swarm.listen_on(
        Multiaddr::empty()
            .with(server_address.ip().into())
            .with(Protocol::Tcp(server_address.port())),
    )?;

    swarm
        .listen_on(
            Multiaddr::empty()
                .with(server_address.ip().into())
                .with(Protocol::Udp(server_address.port()))
                .with(Protocol::QuicV1),
        )
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
    server_address: SocketAddr,
    transport: Transport,
    upload_bytes: Option<usize>,
    download_bytes: Option<usize>,
) -> Result<()> {
    let server_address = match transport {
        Transport::Tcp => Multiaddr::empty()
            .with(server_address.ip().into())
            .with(Protocol::Tcp(server_address.port())),
        Transport::QuicV1 => Multiaddr::empty()
            .with(server_address.ip().into())
            .with(Protocol::Udp(server_address.port()))
            .with(Protocol::QuicV1),
    };

    let benchmarks = if upload_bytes.is_some() {
        vec![custom(
            server_address,
            RunParams {
                to_send: upload_bytes.unwrap(),
                to_receive: download_bytes.unwrap(),
            },
        )
        .boxed()]
    } else {
        vec![
            latency(server_address.clone()).boxed(),
            throughput(server_address.clone()).boxed(),
            requests_per_second(server_address.clone()).boxed(),
            sequential_connections_per_second(server_address.clone()).boxed(),
        ]
    };

    tokio::spawn(async move {
        for benchmark in benchmarks {
            benchmark.await?;
        }

        anyhow::Ok(())
    })
    .await??;

    Ok(())
}

async fn custom(server_address: Multiaddr, params: RunParams) -> Result<()> {
    info!("start benchmark: custom");
    let mut swarm = swarm().await?;

    let start = Instant::now();

    let server_peer_id = connect(&mut swarm, server_address.clone()).await?;

    perf(&mut swarm, server_peer_id, params).await?;

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct CustomResult {
        latency: f64,
    }

    println!(
        "{}",
        serde_json::to_string(&CustomResult {
            latency: start.elapsed().as_secs_f64(),
        })
        .unwrap()
    );

    Ok(())
}

async fn latency(server_address: Multiaddr) -> Result<()> {
    info!("start benchmark: round-trip-time latency");
    let mut swarm = swarm().await?;

    let server_peer_id = connect(&mut swarm, server_address.clone()).await?;

    let mut rounds = 0;
    let start = Instant::now();
    let mut latencies = Vec::new();

    loop {
        if start.elapsed() > Duration::from_secs(30) {
            break;
        }

        let start = Instant::now();

        perf(
            &mut swarm,
            server_peer_id,
            RunParams {
                to_send: 1,
                to_receive: 1,
            },
        )
        .await?;

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
    Ok(())
}

fn percentile<V: PartialOrd + Copy>(values: &[V], percentile: f64) -> V {
    let n: usize = (values.len() as f64 * percentile).ceil() as usize - 1;
    values[n]
}

async fn throughput(server_address: Multiaddr) -> Result<()> {
    info!("start benchmark: single connection single channel throughput");
    let mut swarm = swarm().await?;

    let server_peer_id = connect(&mut swarm, server_address.clone()).await?;

    let params = RunParams {
        to_send: 10 * 1024 * 1024,
        to_receive: 10 * 1024 * 1024,
    };

    perf(&mut swarm, server_peer_id, params).await?;

    Ok(())
}

async fn requests_per_second(server_address: Multiaddr) -> Result<()> {
    info!("start benchmark: single connection parallel requests per second");
    let mut swarm = swarm().await?;

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

    Ok(())
}

async fn sequential_connections_per_second(server_address: Multiaddr) -> Result<()> {
    info!("start benchmark: sequential connections with single request per second");
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

        let mut swarm = swarm().await?;

        let start = Instant::now();

        let server_peer_id = connect(&mut swarm, server_address.clone()).await?;

        latency_connection_establishment.push(start.elapsed().as_secs_f64());

        perf(
            &mut swarm,
            server_peer_id,
            RunParams {
                to_send,
                to_receive,
            },
        )
        .await?;

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

    Ok(())
}

async fn swarm<B: NetworkBehaviour + Default>() -> Result<Swarm<B>> {
    let local_key = libp2p_identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    let transport = {
        let tcp = libp2p_tcp::tokio::Transport::new(libp2p_tcp::Config::default().nodelay(true))
            .upgrade(upgrade::Version::V1Lazy)
            .authenticate(libp2p_tls::Config::new(&local_key)?)
            .multiplex(libp2p_yamux::Config::default());

        let quic = {
            let mut config = libp2p_quic::Config::new(&local_key);
            config.support_draft_29 = true;
            libp2p_quic::tokio::Transport::new(config)
        };

        let dns = libp2p_dns::tokio::Transport::system(OrTransport::new(quic, tcp))?;

        dns.map(|either_output, _| match either_output {
            Either::Left((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
            Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .boxed()
    };

    let swarm = Swarm::new_with_config(
        transport,
        Default::default(),
        local_peer_id,
        Config::with_tokio_executor()
            .with_substream_upgrade_protocol_override(upgrade::Version::V1Lazy),
    );

    Ok(swarm)
}

async fn connect(
    swarm: &mut Swarm<libp2p_perf::client::Behaviour>,
    server_address: Multiaddr,
) -> Result<PeerId> {
    let start = Instant::now();
    swarm.dial(server_address.clone()).unwrap();

    let server_peer_id = match swarm.next().await.unwrap() {
        SwarmEvent::ConnectionEstablished { peer_id, .. } => peer_id,
        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
            bail!("Outgoing connection error to {:?}: {:?}", peer_id, error);
        }
        e => panic!("{e:?}"),
    };

    let duration = start.elapsed();
    let duration_seconds = duration.as_secs_f64();

    info!("established connection in {duration_seconds:.4} s");

    Ok(server_peer_id)
}

async fn perf(
    swarm: &mut Swarm<libp2p_perf::client::Behaviour>,
    server_peer_id: PeerId,
    params: RunParams,
) -> Result<RunDuration> {
    swarm.behaviour_mut().perf(server_peer_id, params)?;

    let duration = match swarm.next().await.unwrap() {
        SwarmEvent::Behaviour(libp2p_perf::client::Event {
            id: _,
            result: Ok(duration),
        }) => duration,
        e => panic!("{e:?}"),
    };

    info!("{}", Run { params, duration });

    Ok(duration)
}
