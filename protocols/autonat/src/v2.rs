use libp2p_swarm::StreamProtocol;

pub mod client;
mod generated;
pub(crate) mod protocol;
pub mod server;

pub(crate) const DIAL_REQUEST_PROTOCOL: StreamProtocol =
    StreamProtocol::new("/libp2p/autonat/2/dial-request");
pub(crate) const DIAL_BACK_PROTOCOL: StreamProtocol =
    StreamProtocol::new("/libp2p/autonat/2/dial-back");

type Nonce = u64;
