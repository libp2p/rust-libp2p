// Copyright 2022 Parity Technologies (UK) Ltd.
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

use std::num::NonZeroUsize;

use futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p_core::{
    UpgradeInfo,
    upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade},
};
use libp2p_identity as identity;
use libp2p_identity::PeerId;
use libp2p_noise as noise;
pub use noise::Error;

use crate::{
    fingerprint::Fingerprint,
    stream::{DEFAULT_MAX_MESSAGE_SIZE, MIN_MESSAGE_SIZE, StreamConfig},
};

pub async fn inbound<T>(
    id_keys: identity::Keypair,
    stream: T,
    client_fingerprint: Fingerprint,
    server_fingerprint: Fingerprint,
) -> Result<PeerId, Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    inbound_with_message_size(
        id_keys,
        stream,
        client_fingerprint,
        server_fingerprint,
        StreamConfig::default(),
    )
    .await
    .map(|(peer_id, _)| peer_id)
}

/// Authenticates the connection and negotiates its encoded message-size limit.
pub async fn inbound_with_message_size<T>(
    id_keys: identity::Keypair,
    stream: T,
    client_fingerprint: Fingerprint,
    server_fingerprint: Fingerprint,
    stream_config: StreamConfig,
) -> Result<(PeerId, StreamConfig), Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let noise = noise::Config::new(&id_keys)
        .unwrap()
        .with_prologue(noise_prologue(client_fingerprint, server_fingerprint));
    let info = noise.protocol_info().next().unwrap();
    // Note the roles are reversed because it allows the server (webrtc connection responder) to
    // send application data 0.5 RTT earlier.
    let (peer_id, mut channel) = noise.upgrade_outbound(stream, info).await?;

    let stream_config = negotiate_message_size(&mut channel, stream_config).await;
    let _ = channel.close().await;

    Ok((peer_id, stream_config))
}

pub async fn outbound<T>(
    id_keys: identity::Keypair,
    stream: T,
    server_fingerprint: Fingerprint,
    client_fingerprint: Fingerprint,
) -> Result<PeerId, Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    outbound_with_message_size(
        id_keys,
        stream,
        server_fingerprint,
        client_fingerprint,
        StreamConfig::default(),
    )
    .await
    .map(|(peer_id, _)| peer_id)
}

/// Authenticates the connection and negotiates its encoded message-size limit.
pub async fn outbound_with_message_size<T>(
    id_keys: identity::Keypair,
    stream: T,
    server_fingerprint: Fingerprint,
    client_fingerprint: Fingerprint,
    stream_config: StreamConfig,
) -> Result<(PeerId, StreamConfig), Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let noise = noise::Config::new(&id_keys)
        .unwrap()
        .with_prologue(noise_prologue(client_fingerprint, server_fingerprint));
    let info = noise.protocol_info().next().unwrap();
    // Note the roles are reversed because it allows the server (webrtc connection responder) to
    // send application data 0.5 RTT earlier.
    let (peer_id, mut channel) = noise.upgrade_inbound(stream, info).await?;

    let stream_config = negotiate_message_size(&mut channel, stream_config).await;
    let _ = channel.close().await;

    Ok((peer_id, stream_config))
}

/// Exchanges the local limit after the authenticated Noise handshake.
///
/// Older peers close the reserved data channel immediately after Noise. Treat that as their
/// historical 16 KiB limit, preserving compatibility while newer peers use the smaller limit.
async fn negotiate_message_size<T>(channel: &mut T, local: StreamConfig) -> StreamConfig
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let fallback = local.limited_by(DEFAULT_MAX_MESSAGE_SIZE);
    let advertised = local.max_message_size() as u64;

    if channel.write_all(&advertised.to_be_bytes()).await.is_err() || channel.flush().await.is_err()
    {
        return fallback;
    }

    let mut remote = [0; std::mem::size_of::<u64>()];
    if channel.read_exact(&mut remote).await.is_err() {
        return fallback;
    }

    effective_message_size(local, Some(u64::from_be_bytes(remote)))
}

fn effective_message_size(local: StreamConfig, remote: Option<u64>) -> StreamConfig {
    let fallback = local.limited_by(DEFAULT_MAX_MESSAGE_SIZE);
    let Some(remote) = remote else {
        return fallback;
    };
    let Ok(remote) = usize::try_from(remote) else {
        return fallback;
    };
    let Some(remote) = NonZeroUsize::new(remote) else {
        return fallback;
    };
    if remote < MIN_MESSAGE_SIZE {
        return fallback;
    }

    local.limited_by(remote)
}

pub(crate) fn noise_prologue(
    client_fingerprint: Fingerprint,
    server_fingerprint: Fingerprint,
) -> Vec<u8> {
    let client = client_fingerprint.to_multihash().to_bytes();
    let server = server_fingerprint.to_multihash().to_bytes();
    const PREFIX: &[u8] = b"libp2p-webrtc-noise:";
    let mut out = Vec::with_capacity(PREFIX.len() + client.len() + server.len());
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(&client);
    out.extend_from_slice(&server);
    out
}

#[cfg(test)]
mod tests {
    use hex_literal::hex;

    use super::*;

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

        assert_eq!(
            hex::encode(prologue1),
            "6c69627032702d7765627274632d6e6f6973653a12203e79af40d6059617a0d83b83a52ce73b0c1f37a72c6043ad2969e2351bdca870122030fc9f469c207419dfdd0aab5f27a86c973c94e40548db9375cca2e915973b99"
        );
        assert_eq!(
            hex::encode(prologue2),
            "6c69627032702d7765627274632d6e6f6973653a122030fc9f469c207419dfdd0aab5f27a86c973c94e40548db9375cca2e915973b9912203e79af40d6059617a0d83b83a52ce73b0c1f37a72c6043ad2969e2351bdca870"
        );
    }

    #[test]
    fn message_size_negotiation_uses_the_smaller_valid_limit() {
        let local = StreamConfig::new(NonZeroUsize::new(16 * 1024).unwrap());

        assert_eq!(
            effective_message_size(local, Some(8 * 1024)).max_message_size(),
            8 * 1024
        );
    }

    #[test]
    fn message_size_negotiation_falls_back_for_legacy_or_invalid_peers() {
        let local = StreamConfig::new(NonZeroUsize::new(8 * 1024).unwrap());

        assert_eq!(
            effective_message_size(local, None).max_message_size(),
            8 * 1024
        );
        assert_eq!(
            effective_message_size(local, Some(0)).max_message_size(),
            8 * 1024
        );
    }
}
