use std::{
    convert::Infallible,
    future::{ready, Ready},
};

use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_swarm::{Stream, StreamProtocol};

pub struct Upgrade {
    pub(crate) supported_protocols: Vec<StreamProtocol>,
}

impl UpgradeInfo for Upgrade {
    type Info = StreamProtocol;

    type InfoIter = std::vec::IntoIter<StreamProtocol>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.supported_protocols.clone().into_iter()
    }
}

impl InboundUpgrade<Stream> for Upgrade {
    type Output = (Stream, StreamProtocol);

    type Error = Infallible;

    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: Stream, info: Self::Info) -> Self::Future {
        ready(Ok((socket, info)))
    }
}

impl OutboundUpgrade<Stream> for Upgrade {
    type Output = (Stream, StreamProtocol);

    type Error = Infallible;

    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: Stream, info: Self::Info) -> Self::Future {
        ready(Ok((socket, info)))
    }
}
