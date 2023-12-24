use std::{io, time::Duration};

use anyhow::{Context, Result};
use futures::{AsyncReadExt, AsyncWriteExt, StreamExt};
use libp2p::{multiaddr::Protocol, Multiaddr, PeerId, Stream, StreamProtocol};
use stream_example as stream;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

const PROTOCOL: StreamProtocol = StreamProtocol::new("/my-ping-protocol");

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env()?,
        )
        .init();

    let maybe_address = std::env::args()
        .next()
        .map(|arg| arg.parse::<Multiaddr>())
        .transpose()
        .context("Failed to parse argument as `Multiaddr`")?;

    let mut swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_quic()
        .with_behaviour(|_| stream::Behaviour::default())?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(10)))
        .build();

    // Register our custom protocol with the behaviour.
    let (control, mut incoming_streams) = swarm.behaviour_mut().register(PROTOCOL);

    // Deal with incoming streams.
    // Spawning a dedicated task is just one way of doing this.
    // libp2p doesn't care how you handle incoming streams but you _must_ handle them somehow.
    // To mitigate DoS attacks, libp2p will internally drop incoming streams if your application cannot keep in processing them.
    tokio::spawn(async move {
        // This loop handles incoming streams _sequentially_ but that doesn't have to be the case.
        // You can also spawn a dedicated task per stream if you want to.
        // Be aware that this breaks backpressure though as spawning new tasks is equivalent to an unbounded buffer.
        // Each task needs memory meaning an aggressive remote peer may force you OOM this way.
        loop {
            let (peer, stream) = incoming_streams.next().await;

            if let Err(e) = inbound_ping(stream).await {
                tracing::warn!(%peer, "Ping protocol failed: {e}");
            }
        }
    });

    // In this demo application, the dialing peer initiates the protocol.
    if let Some(address) = maybe_address {
        let Some(Protocol::P2p(peer_id)) = address.iter().last() else {
            anyhow::bail!("Provided address does not end in `/p2p`");
        };

        swarm.dial(address)?;

        tokio::spawn(connection_handler(peer_id, control));
    }

    // Poll the swarm to make progress.
    loop {
        let event = swarm.next().await.expect("never terminates");

        tracing::trace!(?event);
    }
}

/// A very simple, `async fn`-based connection handler for our custom ping protocol.
///
/// Given a [`PeerId`], it will
async fn connection_handler(peer: PeerId, mut control: stream::Control) {
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await; // Wait a second between pings.

        let stream = match control.open_stream(peer).await {
            Ok(stream) => stream,
            Err(stream::Error::UnsupportedProtocol) => {
                tracing::info!(%peer, %PROTOCOL, "Peer does not support protocol");
                return;
            }
            Err(stream::Error::Io(e)) => {
                // IO errors may be temporary.
                // In production, something like an exponential backoff / circuit-breaker may be more appropriate.
                tracing::debug!(%peer, "IO error when opening stream: {e}");
                continue;
            }
        };

        if let Err(e) = outbound_ping(stream).await {
            tracing::warn!(%peer, "Ping protocol failed: {e}");
        }
    }
}

async fn inbound_ping(mut stream: Stream) -> io::Result<()> {
    let mut ping = [0u8; 32];
    stream.read_exact(&mut ping).await?;
    stream.write_all(&ping).await?;
    stream.close().await?;

    Ok(())
}

async fn outbound_ping(mut stream: Stream) -> io::Result<()> {
    let ping = rand::random::<[u8; 32]>();
    stream.write_all(&ping).await?;
    stream.close().await?;

    let mut pong = [0u8; 32];
    stream.read_exact(&mut pong).await?;

    if ping != pong {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "ping payload mismatch",
        ));
    }

    Ok(())
}
