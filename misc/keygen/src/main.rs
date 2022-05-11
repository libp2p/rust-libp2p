use std::error::Error;
use std::path::PathBuf;
use std::str::{self, FromStr};
use std::sync::mpsc;
use std::thread;

mod config;

use libp2p_core::identity::{self, ed25519};
use libp2p_core::PeerId;
use structopt::StructOpt;
use zeroize::Zeroizing;

#[derive(Debug, StructOpt)]
#[structopt(name = "libp2p key material generator")]
struct Args {
    /// JSON formatted output
    #[structopt(long, global = true)]
    json: bool,

    #[structopt(subcommand)]
    cmd: Command,
}

#[derive(Debug, StructOpt)]
enum Command {
    /// Read from config file
    From {
        /// Provide a IPFS config file
        #[structopt(parse(from_os_str))]
        config: PathBuf,
    },
    /// Generate random
    Rand {
        /// The keypair prefix
        #[structopt(long)]
        prefix: Option<String>,
    },
}

// Due to the fact that a peer id uses a SHA-256 multihash, it always starts with the
// bytes 0x1220, meaning that only some characters are valid.
const ALLOWED_FIRST_BYTE: &[u8] = b"NPQRSTUVWXYZ";

// The base58 alphabet is not necessarily obvious.
const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::from_args();

    let (local_peer_id, local_keypair) = match args.cmd {
        // Generate keypair from some sort of key material. Currently supporting `IPFS` config file
        Command::From { config } => {
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
        Command::Rand { prefix } => {
            if let Some(prefix) = prefix {
                if prefix.as_bytes().iter().any(|c| !ALPHABET.contains(c)) {
                    eprintln!("Prefix {} is not valid base58", prefix);
                    std::process::exit(1);
                }

                // Checking conformity to ALLOWED_FIRST_BYTE.
                if !prefix.is_empty() && !ALLOWED_FIRST_BYTE.contains(&prefix.as_bytes()[0]) {
                    eprintln!("Prefix {} is not reachable", prefix);
                    eprintln!(
                        "Only the following bytes are possible as first byte: {}",
                        str::from_utf8(ALLOWED_FIRST_BYTE).unwrap()
                    );
                    std::process::exit(1);
                }

                let (tx, rx) = mpsc::channel::<(PeerId, identity::Keypair)>();

                // Find peer IDs in a multithreaded fashion.
                for _ in 0..thread::available_parallelism()?.get() {
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
            } else {
                let keypair = identity::Keypair::Ed25519(ed25519::Keypair::generate());
                (keypair.public().into(), keypair)
            }
        }
    };

    if args.json {
        let config = config::Config::from_key_material(local_peer_id, &local_keypair)?;
        println!("{}", serde_json::to_string(&config)?);
    } else {
        println!(
            "PeerId: {:?} Keypair: {:?}",
            local_peer_id,
            local_keypair.to_protobuf_encoding()
        );
    }

    Ok(())
}
