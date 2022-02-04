use std::error::Error;
use std::str::{self, FromStr};
use std::sync::mpsc;
use std::thread;

mod cli;
mod config;

use cli::{FromCmd, RandCmd, Subcommand};
use libp2p::identity::{self, ed25519};
use libp2p::PeerId;
use zeroize::Zeroizing;

// Due to the fact that a peer id uses a SHA-256 multihash, it always starts with the
// bytes 0x1220, meaning that only some characters are valid.
const ALLOWED_FIRST_BYTE: &[u8] = b"NPQRSTUVWXYZ";

// The base58 alphabet is not necessarily obvious.
const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn main() -> Result<(), Box<dyn Error>> {
    let args: cli::Cli = argh::from_env();

    let (local_peer_id, local_keypair) = match args.subcommand() {
        // Generate keypair from some sort of key material. Currently supporting `IPFS` config file
        Subcommand::From(FromCmd { config }) => {
            let config = Zeroizing::new(config::Config::from_file(config.as_ref())?);

            let keypair = identity::Keypair::from_protobuf_encoding(&Zeroizing::new(
                base64::decode(config.identity.priv_key.as_bytes())?,
            ))?;

            let peer_id = keypair.public().into();
            assert_eq!(
                    PeerId::from_str(&config.identity.peer_id)?,
                    peer_id,
                    "Expect peer id derived from private key and peer id retrieved from config to match."
                );

            (peer_id, keypair)
        }

        // Generate a random keypair, optionally with a prefix
        Subcommand::Random(RandCmd { prefix, .. }) => {
            if prefix.is_empty() {
                let keypair = identity::Keypair::Ed25519(ed25519::Keypair::generate());
                (keypair.public().into(), keypair)
            } else {
                if prefix.as_bytes().iter().any(|c| !ALPHABET.contains(c)) {
                    eprintln!("Prefix {} is not valid base58", prefix);
                    std::process::exit(1); // error
                }

                // Checking conformity to ALLOWED_FIRST_BYTE.
                if !prefix.is_empty() && !ALLOWED_FIRST_BYTE.contains(&prefix.as_bytes()[0]) {
                    eprintln!("Prefix {} is not reachable", prefix);
                    eprintln!(
                        "Only the following bytes are possible as first byte: {}",
                        str::from_utf8(ALLOWED_FIRST_BYTE).unwrap()
                    );
                    std::process::exit(1); // error
                }

                let (tx, rx) = mpsc::channel::<(PeerId, identity::Keypair)>();

                // Find peer IDs in a multithreaded fashion.
                for _ in 0..num_cpus::get() {
                    let prefix = prefix.clone();
                    let tx = tx.clone();

                    thread::spawn(move || loop {
                        let keypair = ed25519::Keypair::generate();
                        let peer_id = identity::PublicKey::Ed25519(keypair.public()).to_peer_id();
                        let base58 = peer_id.to_base58();
                        if base58[8..].starts_with(&prefix) {
                            tx.send((peer_id, identity::Keypair::Ed25519(keypair)))
                                .expect("to send");
                        }
                    });
                }

                rx.recv().expect("to recv")
            }
        }
    };

    println!(
        "PeerId: {:?} Keypair: {:?}",
        local_peer_id,
        local_keypair.to_protobuf_encoding()
    );

    Ok(())
}
