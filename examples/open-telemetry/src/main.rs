// Copyright 2018 Parity Technologies (UK) Ltd.
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

use libp2p::core::transport::upgrade;
use libp2p::mdns;
use libp2p::swarm::NetworkBehaviour;
use opentelemetry::sdk;
use opentelemetry::sdk::export::metrics;
use opentelemetry_otlp::WithExportConfig;
use std::error::Error;
use tracing::Dispatch;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

use futures::prelude::*;
use libp2p::{
    identity, noise, ping,
    swarm::{SwarmBuilder, SwarmEvent},
    tcp, yamux, PeerId, Transport,
};
use std::time::Duration;

// We create a custom network behaviour that combines Ping and Mdns.
#[derive(NetworkBehaviour)]
struct MyBehaviour {
    ping: ping::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

async fn init_telemetry() -> Result<(), Box<dyn Error>> {
    let grpc_endpoint = format!("http://localhost:4317");

    tracing::trace!(%grpc_endpoint, "Setting up OTLP exporter for collector");

    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(grpc_endpoint.clone());

    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(sdk::trace::Config::default())
        .install_batch(opentelemetry::runtime::Tokio)?;

    tracing::trace!("Successfully initialized trace provider on tokio runtime");

    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(grpc_endpoint);

    opentelemetry_otlp::new_pipeline()
        .metrics(
            sdk::metrics::selectors::simple::inexpensive(),
            metrics::aggregation::cumulative_temporality_selector(),
            opentelemetry::runtime::Tokio,
        )
        .with_exporter(exporter)
        .build()?;

    tracing::trace!("Successfully initialized metric controller on tokio runtime");

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let dispatch: Dispatch = tracing_subscriber::registry()
        .with(
            tracing_opentelemetry::layer()
                .with_tracer(tracer)
                .with_filter(env_filter),
        )
        .into();

    let _ = dispatch.try_init();

    Ok(())
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .try_init();
    
    init_telemetry().await?;

    let local_key = identity::Keypair::generate_ed25519();
    let local_peer_id = PeerId::from(local_key.public());

    // Set up an encrypted DNS-enabled TCP Transport over the yamux protocol.
    let tcp_transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
        .upgrade(upgrade::Version::V1Lazy)
        .authenticate(noise::Config::new(&local_key)?)
        .multiplex(yamux::Config::default())
        .timeout(std::time::Duration::from_secs(20))
        .boxed();

    let mut swarm = {
        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?;
        let behaviour = MyBehaviour {
            mdns,
            ping: ping::Behaviour::default(),
        };
        SwarmBuilder::with_tokio_executor(tcp_transport, behaviour, local_peer_id)
            .idle_connection_timeout(Duration::from_secs(60))
            .build()
    };

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => println!("Listening on {address:?}"),
            SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                for (_, addr) in list {
                    swarm.dial(addr.clone())?;
                    println!("Dialed {addr}")
                }
            }
            SwarmEvent::Behaviour(event) => println!("{event:?}"),
            _ => {}
        }
    }
}
