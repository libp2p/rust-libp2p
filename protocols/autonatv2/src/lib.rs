use libp2p_core::upgrade::ReadyUpgrade;
use libp2p_swarm::StreamProtocol;

pub mod client;
mod generated;
mod global_only;
pub(crate) mod request_response;
pub mod server;

pub(crate) const REQUEST_PROTOCOL_NAME: StreamProtocol =
    StreamProtocol::new("/libp2p/autonat/2/dial-request");
pub(crate) const DIAL_BACK_PROTOCOL_NAME: StreamProtocol =
    StreamProtocol::new("/libp2p/autonat/2/dial-back");
pub(crate) const REQUEST_UPGRADE: ReadyUpgrade<StreamProtocol> =
    ReadyUpgrade::new(REQUEST_PROTOCOL_NAME);
pub(crate) const DIAL_BACK_UPGRADE: ReadyUpgrade<StreamProtocol> =
    ReadyUpgrade::new(DIAL_BACK_PROTOCOL_NAME);

pub type Nonce = u64;
