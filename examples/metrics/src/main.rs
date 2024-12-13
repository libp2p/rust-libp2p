// Copyright 2021 Protocol Labs.
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

#![doc = include_str!("../README.md")]

use std::error::Error;

use futures::StreamExt;
use libp2p::{
    core::Multiaddr,
    identify, identity,
    metrics::{Metrics, Recorder},
    noise, ping,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};
use opentelemetry::{trace::TracerProvider as _, KeyValue};
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::{runtime, trace::TracerProvider};
use prometheus_client::registry::Registry;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

mod http_service;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    setup_tracing()?;

    let mut metric_registry = Registry::default();

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_bandwidth_metrics(&mut metric_registry)
        .with_behaviour(|key| Behaviour::new(key.public()))?
        .build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    if let Some(addr) = std::env::args().nth(1) {
        let remote: Multiaddr = addr.parse()?;
        swarm.dial(remote)?;
        tracing::info!(address=%addr, "Dialed address")
    }

    let metrics = Metrics::new(&mut metric_registry);
    tokio::spawn(http_service::metrics_server(metric_registry));

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                tracing::info!(
                    "Local node is listening on\n {}/p2p/{}",
                    address,
                    swarm.local_peer_id()
                );
            }
            SwarmEvent::Behaviour(BehaviourEvent::Ping(ping_event)) => {
                tracing::info!(?ping_event);
                metrics.record(&ping_event);
            }
            SwarmEvent::Behaviour(BehaviourEvent::Identify(identify_event)) => {
                tracing::info!(?identify_event);
                metrics.record(&identify_event);
            }
            swarm_event => {
                tracing::info!(?swarm_event);
                metrics.record(&swarm_event);
            }
        }
    }
}

fn setup_tracing() -> Result<(), Box<dyn Error>> {
    let provider = TracerProvider::builder()
        .with_batch_exporter(
            SpanExporter::builder().with_tonic().build()?,
            runtime::Tokio,
        )
        .with_resource(opentelemetry_sdk::Resource::new(vec![KeyValue::new(
            "service.name",
            "libp2p",
        )]))
        .build();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(EnvFilter::from_default_env()))
        .with(
            tracing_opentelemetry::layer()
                .with_tracer(provider.tracer("libp2p-subscriber"))
                .with_filter(EnvFilter::from_default_env()),
        )
        .init();

    Ok(())
}

/// Our network behaviour.
#[derive(NetworkBehaviour)]
struct Behaviour {
    identify: identify::Behaviour,
    ping: ping::Behaviour,
}

impl Behaviour {
    fn new(local_pub_key: identity::PublicKey) -> Self {
        Self {
            ping: ping::Behaviour::default(),
            identify: identify::Behaviour::new(identify::Config::new(
                "/ipfs/0.1.0".into(),
                local_pub_key,
            )),
        }
    }
}
