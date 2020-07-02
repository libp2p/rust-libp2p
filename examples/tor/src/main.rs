#![warn(rust_2018_idioms)]
#![forbid(unsafe_code)]
use anyhow::{Context, Result};
use log::{warn, Level};
use structopt::StructOpt;

use ping_pong::{run_dialer, run_listener, Opt};

/// The ping-pong onion service address.
const ONION: &str = "/onion3/r4nttccifklkruvrztwxuhk2iy4xx7cnnex2sgogbo4zw6rnx3cq2bid:7";

/// Tor should be started with a hidden service configured. Must be
/// re-started for each execution of the listener.
///
/// See torrc for an example, if using that file Tor can be started with:
///
///   sudo /usr/bin/tor --defaults-torrc tor-service-defaults-torrc -f torrc --RunAsDaemon 0
///
/// After the first execution of Tor the onion address can be found in
///
///   /var/lib/tor/hidden_service/hostname
///
/// Update ONION above before running the listener.
#[tokio::main]
async fn main() -> Result<()> {
    simple_logger::init_with_level(Level::Debug).unwrap();

    let opt = Opt::from_args();

    let addr = opt.onion.unwrap_or_else(|| ONION.to_string());
    let addr = addr
        .parse()
        .with_context(|| format!("failed to parse multiaddr: {}", addr))?;

    if opt.dialer {
        run_dialer(addr).await?;
    } else {
        run_listener(addr).await?;
    }

    Ok(())
}
