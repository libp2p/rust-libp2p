use libp2p_core::upgrade::ReadyUpgrade;
use libp2p_swarm::StreamProtocol;

mod client;
mod generated;
mod global_only;
pub(crate) mod request_response;

pub(crate) const REQUEST_PROTOCOL_NAME: StreamProtocol =
    StreamProtocol::new("/libp2p/autonat/2/dial-request");
pub(crate) const DIAL_BACK_PROTOCOL_NAME: StreamProtocol =
    StreamProtocol::new("/libp2p/autonat/2/dial-back");
pub(crate) const REQUEST_UPGRADE: ReadyUpgrade<StreamProtocol> =
    ReadyUpgrade::new(REQUEST_PROTOCOL_NAME);
pub(crate) const DIAL_BACK_UPGRADE: ReadyUpgrade<StreamProtocol> =
    ReadyUpgrade::new(DIAL_BACK_PROTOCOL_NAME);

pub(crate) use request_response::DATA_FIELD_LEN_UPPER_BOUND;
pub(crate) use request_response::DATA_LEN_LOWER_BOUND;
pub(crate) use request_response::DATA_LEN_UPPER_BOUND;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
