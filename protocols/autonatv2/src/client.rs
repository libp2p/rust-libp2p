use libp2p_core::Multiaddr;

use crate::request_response::{DialRequest, DialResponse};

mod behaviour;
mod handler;

#[derive(Debug)]
enum ToBehaviour<E> {
    ResponseInfo(ResponseInfo),
    OutboundError(E),
}

#[derive(Debug)]
struct ResponseInfo {
    response: DialResponse,
    suspicious_addrs: Vec<Multiaddr>,
    successfull_addr: Option<Multiaddr>,
}

impl ResponseInfo {
    fn new(
        response: DialResponse,
        suspicious_addrs: Vec<Multiaddr>,
        successfull_addr: Option<Multiaddr>,
    ) -> Self {
        Self {
            response,
            suspicious_addrs,
            successfull_addr,
        }
    }
}

#[derive(Debug)]
enum FromBehaviour {
    Dial(DialRequest),
}
