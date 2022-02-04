use std::ffi::OsString;

use argh::FromArgs;

#[derive(FromArgs, PartialEq, Debug)]
/// Libp2p keygen
pub struct Cli {
    #[argh(subcommand)]
    subcommand: Subcommand,
}

impl Cli {
    pub fn subcommand(&self) -> &Subcommand {
        &self.subcommand
    }
}

#[derive(FromArgs, PartialEq, Debug)]
#[argh(subcommand)]
pub enum Subcommand {
    Random(RandCmd),
    From(FromCmd),
}

#[derive(FromArgs, PartialEq, Debug)]
/// generate random key material
#[argh(subcommand, name = "rand")]
pub struct RandCmd {
    #[argh(option, default = "String::new()")]
    /// prefix
    pub prefix: String,

    #[argh(option, default = "String::from(\"ed25519\")")]
    /// encryption type
    pub r#type: String,
}

#[derive(FromArgs, PartialEq, Debug)]
/// generate key material from ...
#[argh(subcommand, name = "from")]
pub struct FromCmd {
    /// generate keypair from `IPFS` config file
    #[argh(option)]
    pub config: OsString,
}
