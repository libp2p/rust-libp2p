use std::error::Error;

use libp2p::{
    dns, identity, mplex, noise,
    ping::{self, Behaviour},
    swarm::SwarmEvent,
    tcp, yamux, PeerId, Swarm, Transport,
};
use libp2p_onion::OnionClient;

#[cfg(feature = "async-std")]
use async_std_crate as async_std;

// a sample onion transport setup
#[allow(unused)]
#[cfg(feature = "async-std")]
async fn onion_transport(
    keypair: identity::Keypair,
) -> Result<
    libp2p_core::transport::Boxed<(PeerId, libp2p_core::muxing::StreamMuxerBox)>,
    Box<dyn Error>,
> {
    use std::time::Duration;

    use libp2p_core::upgrade::{SelectUpgrade, Version};

    let transport = {
        let dns_tcp = dns::DnsConfig::system(tcp::TcpTransport::new(
            tcp::GenTcpConfig::new().nodelay(true),
        ))
        .await?;
        let onion = OnionClient::from_builder(OnionClient::builder())?;
        println!("bootstrapping...");
        onion.bootstrap().await?;
        println!("bootstrapped!");
        onion.or_transport(dns_tcp)
    };

    let noise_keys = noise::Keypair::<noise::X25519Spec>::new()
        .into_authentic(&keypair)
        .expect("Signing libp2p-noise static DH keypair failed.");
    Ok(transport
        .upgrade(Version::V1)
        .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
        .multiplex(SelectUpgrade::new(
            yamux::YamuxConfig::default(),
            mplex::MplexConfig::default(),
        ))
        .timeout(Duration::from_secs(20))
        .boxed())
}

#[cfg(feature = "async-std")]
async fn onion_ping_swarm(keypair: identity::Keypair) -> Result<Swarm<Behaviour>, Box<dyn Error>> {
    let local_peer_id = PeerId::from(keypair.public());
    let transport = onion_transport(keypair).await?;

    let behaviour = ping::Behaviour::new(ping::Config::new().with_keep_alive(true));

    let swarm = Swarm::new(transport, behaviour, local_peer_id);
    Ok(swarm)
}

#[cfg(feature = "async-std")]
#[async_std_crate::test]
async fn try_listen() {
    let local_key = identity::Keypair::generate_ed25519();

    let mut swarm = onion_ping_swarm(local_key)
        .await
        .expect("error while creating swarm");

    swarm
        .listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap())
        .expect("listen on failed");

    use futures::prelude::*;

    loop {
        match swarm.select_next_some().await {
            SwarmEvent::NewListenAddr { address, .. } => {
                println!("Listening on {:?}", address);
                break;
            }
            _ => {}
        }
    }
}
