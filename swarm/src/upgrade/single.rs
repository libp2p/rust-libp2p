use crate::{Stream, StreamProtocol};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use std::future::{ready, Ready};
use std::iter;
use void::Void;

pub struct SingleProtocol {
    p: StreamProtocol,
}

impl SingleProtocol {
    pub fn new(p: StreamProtocol) -> Self {
        Self { p }
    }
}

impl UpgradeInfo for SingleProtocol {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.p.clone())
    }
}

impl InboundUpgrade<Stream> for SingleProtocol {
    type Output = Stream;
    type Error = Void;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: Stream, _: Self::Info) -> Self::Future {
        ready(Ok(socket))
    }
}

impl OutboundUpgrade<Stream> for SingleProtocol {
    type Output = Stream;
    type Error = Void;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: Stream, _: Self::Info) -> Self::Future {
        ready(Ok(socket))
    }
}
