use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(name = "ping-pong", about = "libp2p ping-pong application over Tor.")]
pub struct Opt {
    /// Run as the dialer i.e., do the ping
    #[structopt(short, long)]
    pub dialer: bool,

    /// Run as the listener i.e., do the pong (default)
    #[structopt(short, long)]
    pub listener: bool,

    /// Onion mulitaddr to use (only required for dialer)
    #[structopt(long)]
    pub onion: Option<String>,
}
