use std::error::Error;

use libp2p::*;
use libp2p_onion::OnionClient;

#[cfg(feature = "async-std")]
use async_std_crate as async_std;

#[cfg(feature ="async-std")]
#[async_std_crate::test]
async fn onion_transport(keypair: identity::Keypair) -> Result<(), Box<dyn Error>> {
    let transport = {
        let dns_tcp = dns::DnsConfig::system(tcp::TcpTransport::new(
            tcp::GenTcpConfig::new().nodelay(true),
        ))
        .await?;
        let onion = OnionClient::from_builder(OnionClient::builder())?;
        onion.bootstrap().await?;
        dns_tcp.or_transport(onion)
    };

    let noise_keys = noise::Keypair::<noise::X25519Spec>::new()
        .expect("Signing libp2p-noise static DH keypair failed.");

    Ok(())
}
