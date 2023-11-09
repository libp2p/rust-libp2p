use crate::{Stream, StreamProtocol};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use std::future::{pending, Pending};
use std::iter;
use void::Void;

#[derive(Debug, Clone, Copy)]
pub struct NoProtocols {}

impl NoProtocols {
    pub fn new() -> Self {
        Self {}
    }
}

impl UpgradeInfo for NoProtocols {
    type Info = StreamProtocol;
    type InfoIter = iter::Empty<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::empty()
    }
}

impl InboundUpgrade<Stream> for NoProtocols {
    type Output = Void;
    type Error = Void;
    type Future = Pending<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, _: Stream, _: Self::Info) -> Self::Future {
        pending()
    }
}

impl OutboundUpgrade<Stream> for NoProtocols {
    type Output = Void;
    type Error = Void;
    type Future = Pending<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, _: Stream, _: Self::Info) -> Self::Future {
        pending()
    }
}
