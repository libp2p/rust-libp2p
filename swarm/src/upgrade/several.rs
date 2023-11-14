use crate::{Stream, StreamProtocol};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use std::future::{ready, Ready};
use std::vec;
use void::Void;

pub struct SeveralProtocols {
    protocols: Vec<StreamProtocol>,
}

impl SeveralProtocols {
    pub fn new(protocols: Vec<StreamProtocol>) -> Self {
        Self { protocols }
    }
}

impl UpgradeInfo for SeveralProtocols {
    type Info = StreamProtocol;
    type InfoIter = vec::IntoIter<StreamProtocol>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.protocols.clone().into_iter()
    }
}

impl InboundUpgrade<Stream> for SeveralProtocols {
    type Output = (Stream, StreamProtocol);
    type Error = Void;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: Stream, protocol: Self::Info) -> Self::Future {
        ready(Ok((socket, protocol)))
    }
}

impl OutboundUpgrade<Stream> for SeveralProtocols {
    type Output = (Stream, StreamProtocol);
    type Error = Void;
    type Future = Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: Stream, protocol: Self::Info) -> Self::Future {
        ready(Ok((socket, protocol)))
    }
}
