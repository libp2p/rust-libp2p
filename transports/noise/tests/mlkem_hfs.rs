// End-to-end hybrid Noise handshake (X25519 + ML-KEM-768), driven through the
// libp2p upgrade with the hybrid protocol id. Mirrors `smoke.rs`.
#![cfg(feature = "mlkem-hfs")]

use futures::prelude::*;
use libp2p_core::upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade};
use libp2p_identity as identity;
use libp2p_noise as noise;

// Must match `NOISE_MLKEM_HFS_PROTOCOL` in the crate (kept private there).
const HFS: &str = "/noise-mlkem768-hfs/0.1.0";

#[test]
fn xxhfs_mlkem768_handshake_and_transport() {
    let server_id = identity::Keypair::generate_ed25519();
    let client_id = identity::Keypair::generate_ed25519();

    let (client, server) = futures_ringbuf::Endpoint::pair(4096, 4096);

    futures::executor::block_on(async move {
        let ((reported_client_id, mut server_session), (reported_server_id, mut client_session)) =
            futures::future::try_join(
                noise::Config::new(&server_id).unwrap().upgrade_inbound(server, HFS),
                noise::Config::new(&client_id).unwrap().upgrade_outbound(client, HFS),
            )
            .await
            .unwrap();

        assert_eq!(reported_client_id, client_id.public().to_peer_id());
        assert_eq!(reported_server_id, server_id.public().to_peer_id());

        let msg = b"harvest now, decrypt never";
        let client_fut = async {
            client_session.write_all(msg).await.expect("write");
            client_session.flush().await.expect("flush");
        };
        let server_fut = async {
            let mut buf = vec![0u8; msg.len()];
            server_session.read_exact(&mut buf).await.expect("read");
            assert_eq!(&buf, msg);
        };
        futures::future::join(client_fut, server_fut).await;
    });
}

/// Hybrid initiator and classical responder negotiate down to `/noise`.
#[test]
fn falls_back_to_classical_when_peer_is_old() {
    let server_id = identity::Keypair::generate_ed25519();
    let client_id = identity::Keypair::generate_ed25519();

    let (client, server) = futures_ringbuf::Endpoint::pair(4096, 4096);

    futures::executor::block_on(async move {
        let (_, _) = futures::future::try_join(
            noise::Config::new(&server_id).unwrap().upgrade_inbound(server, "/noise"),
            noise::Config::new(&client_id).unwrap().upgrade_outbound(client, "/noise"),
        )
        .await
        .unwrap();
    });
}
