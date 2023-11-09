use crate::upgrade::UpgradeInfoSend;
use crate::StreamProtocol;
use std::iter;

pub struct OneProtocol {
    p: StreamProtocol,
}

impl UpgradeInfoSend for OneProtocol {
    type Info = StreamProtocol;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(self.p.clone())
    }
}
