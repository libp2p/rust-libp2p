use std::collections::HashSet;

use libp2p_core::upgrade::{InboundConnectionUpgrade, OutboundConnectionUpgrade};
use libp2p_identity as identity;
use libp2p_noise as noise;
use multihash::Multihash;

const SHA_256_MH: u64 = 0x12;

#[test]
fn webtransport_same_set_of_certhashes() {
    let (certhash1, certhash2, _) = certhashes();

    handshake_with_certhashes(vec![certhash1, certhash2], vec![certhash1, certhash2]).unwrap();
}

#[test]
fn webtransport_subset_of_certhashes() {
    let (certhash1, certhash2, _) = certhashes();

    handshake_with_certhashes(vec![certhash1], vec![certhash1, certhash2]).unwrap();
}

#[test]
fn webtransport_client_without_certhashes() {
    let (certhash1, certhash2, _) = certhashes();

    // Valid when server uses CA-signed TLS certificate.
    handshake_with_certhashes(vec![], vec![certhash1, certhash2]).unwrap();
}

#[test]
fn webtransport_client_and_server_without_certhashes() {
    // Valid when server uses CA-signed TLS certificate.
    handshake_with_certhashes(vec![], vec![]).unwrap();
}

#[test]
fn webtransport_server_empty_certhashes() {
    let (certhash1, certhash2, _) = certhashes();

    // Invalid case, because a MITM attacker may strip certificates of the server.
    let Err(noise::Error::UnknownWebTransportCerthashes(expected, received)) =
        handshake_with_certhashes(vec![certhash1, certhash2], vec![])
    else {
        panic!("unexpected result");
    };

    assert_eq!(expected, HashSet::from([certhash1, certhash2]));
    assert_eq!(received, HashSet::new());
}

#[test]
fn webtransport_client_uninit_certhashes() {
    let (certhash1, certhash2, _) = certhashes();

    // Valid when server uses CA-signed TLS certificate.
    handshake_with_certhashes(None, vec![certhash1, certhash2]).unwrap();
}

#[test]
fn webtransport_client_and_server_uninit_certhashes() {
    // Valid when server uses CA-signed TLS certificate.
    handshake_with_certhashes(None, None).unwrap();
}

#[test]
fn webtransport_server_uninit_certhashes() {
    let (certhash1, certhash2, _) = certhashes();

    // Invalid case, because a MITM attacker may strip certificates of the server.
    let Err(noise::Error::UnknownWebTransportCerthashes(expected, received)) =
        handshake_with_certhashes(vec![certhash1, certhash2], None)
    else {
        panic!("unexpected result");
    };

    assert_eq!(expected, HashSet::from([certhash1, certhash2]));
    assert_eq!(received, HashSet::new());
}

#[test]
fn webtransport_different_server_certhashes() {
    let (certhash1, certhash2, certhash3) = certhashes();

    let Err(noise::Error::UnknownWebTransportCerthashes(expected, received)) =
        handshake_with_certhashes(vec![certhash1, certhash3], vec![certhash1, certhash2])
    else {
        panic!("unexpected result");
    };

    assert_eq!(expected, HashSet::from([certhash1, certhash3]));
    assert_eq!(received, HashSet::from([certhash1, certhash2]));
}

#[test]
fn webtransport_superset_of_certhashes() {
    let (certhash1, certhash2, _) = certhashes();

    let Err(noise::Error::UnknownWebTransportCerthashes(expected, received)) =
        handshake_with_certhashes(vec![certhash1, certhash2], vec![certhash1])
    else {
        panic!("unexpected result");
    };

    assert_eq!(expected, HashSet::from([certhash1, certhash2]));
    assert_eq!(received, HashSet::from([certhash1]));
}

fn certhashes() -> (Multihash<64>, Multihash<64>, Multihash<64>) {
    (
        Multihash::wrap(SHA_256_MH, b"1").unwrap(),
        Multihash::wrap(SHA_256_MH, b"2").unwrap(),
        Multihash::wrap(SHA_256_MH, b"3").unwrap(),
    )
}

// `valid_certhases` must be a strict subset of `server_certhashes`.
fn handshake_with_certhashes(
    valid_certhases: impl Into<Option<Vec<Multihash<64>>>>,
    server_certhashes: impl Into<Option<Vec<Multihash<64>>>>,
) -> Result<(), noise::Error> {
    let valid_certhases = valid_certhases.into();
    let server_certhashes = server_certhashes.into();

    let client_id = identity::Keypair::generate_ed25519();
    let server_id = identity::Keypair::generate_ed25519();

    let (client, server) = futures_ringbuf::Endpoint::pair(100, 100);

    futures::executor::block_on(async move {
        let mut client_config = noise::Config::new(&client_id)?;
        let mut server_config = noise::Config::new(&server_id)?;

        if let Some(valid_certhases) = valid_certhases {
            client_config =
                client_config.with_webtransport_certhashes(valid_certhases.into_iter().collect());
        }

        if let Some(server_certhashes) = server_certhashes {
            server_config =
                server_config.with_webtransport_certhashes(server_certhashes.into_iter().collect());
        }

        let ((reported_client_id, mut _server_session), (reported_server_id, mut _client_session)) =
            futures::future::try_join(
                server_config.upgrade_inbound(server, ""),
                client_config.upgrade_outbound(client, ""),
            )
            .await?;

        assert_eq!(reported_client_id, client_id.public().to_peer_id());
        assert_eq!(reported_server_id, server_id.public().to_peer_id());

        Ok(())
    })
}
