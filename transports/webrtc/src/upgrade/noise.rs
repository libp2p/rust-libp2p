use crate::fingerprint::Fingerprint;
use crate::Error;
use futures::{AsyncRead, AsyncWrite, AsyncWriteExt};
use libp2p_core::{identity, InboundUpgrade, OutboundUpgrade, PeerId, UpgradeInfo};
use libp2p_noise::{Keypair, NoiseConfig, X25519Spec};
use multihash::Multihash;

pub async fn outbound<T>(
    id_keys: identity::Keypair,
    stream: T,
    our_fingerprint: Fingerprint,
    remote_fingerprint: Fingerprint,
) -> Result<PeerId, Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let dh_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .unwrap();
    let noise =
        NoiseConfig::xx(dh_keys).with_prologue(noise_prologue(our_fingerprint, remote_fingerprint));
    let info = noise.protocol_info().next().unwrap();
    let (peer_id, mut channel) = noise
        .into_authenticated()
        .upgrade_outbound(stream, info)
        .await?;

    channel.close().await?;

    Ok(peer_id)
}

pub async fn inbound<T>(
    id_keys: identity::Keypair,
    stream: T,
    our_fingerprint: Fingerprint,
    remote_fingerprint: Fingerprint,
) -> Result<PeerId, Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let dh_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .unwrap();
    let noise =
        NoiseConfig::xx(dh_keys).with_prologue(noise_prologue(our_fingerprint, remote_fingerprint));
    let info = noise.protocol_info().next().unwrap();
    let (peer_id, mut channel) = noise
        .into_authenticated()
        .upgrade_inbound(stream, info)
        .await?;

    channel.close().await?;

    Ok(peer_id)
}

pub fn noise_prologue(our_fingerprint: Fingerprint, remote_fingerprint: Fingerprint) -> Vec<u8> {
    let (a, b): (Multihash, Multihash) = (
        our_fingerprint.to_multi_hash(),
        remote_fingerprint.to_multi_hash(),
    );
    let (a, b) = (a.to_bytes(), b.to_bytes());
    let (first, second) = if a < b { (a, b) } else { (b, a) };
    const PREFIX: &[u8] = b"libp2p-webrtc-noise:";
    let mut out = Vec::with_capacity(PREFIX.len() + first.len() + second.len());
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(&first);
    out.extend_from_slice(&second);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn noise_prologue_tests() {
        let a = Fingerprint::raw(hex!(
            "3e79af40d6059617a0d83b83a52ce73b0c1f37a72c6043ad2969e2351bdca870"
        ));
        let b = Fingerprint::raw(hex!(
            "30fc9f469c207419dfdd0aab5f27a86c973c94e40548db9375cca2e915973b99"
        ));

        let prologue1 = noise_prologue(a, b);
        let prologue2 = noise_prologue(b, a);

        assert_eq!(hex::encode(&prologue1), "6c69627032702d7765627274632d6e6f6973653a122030fc9f469c207419dfdd0aab5f27a86c973c94e40548db9375cca2e915973b9912203e79af40d6059617a0d83b83a52ce73b0c1f37a72c6043ad2969e2351bdca870");
        assert_eq!(
            prologue1, prologue2,
            "order of fingerprints does not matter"
        );
    }
}
