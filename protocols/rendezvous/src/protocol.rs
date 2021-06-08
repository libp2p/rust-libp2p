use crate::codec::RendezvousCodec;
use asynchronous_codec::Framed;
use futures::AsyncRead;
use futures::AsyncWrite;
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use log::debug;
use std::{future, iter};
use void::Void;

pub fn new() -> Rendezvous {
    Rendezvous::new()
}

#[derive(Default, Debug, Copy, Clone)]
pub struct Rendezvous;

impl Rendezvous {
    fn new() -> Rendezvous {
        Rendezvous
    }
}

impl UpgradeInfo for Rendezvous {
    type Info = &'static [u8];
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once(b"/rendezvous/1.0.0")
    }
}

impl<TSocket: AsyncRead + AsyncWrite> InboundUpgrade<TSocket> for Rendezvous {
    type Output = Framed<TSocket, RendezvousCodec>;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        debug!("upgrading inbound");
        future::ready(Ok(Framed::new(socket, RendezvousCodec::default())))
    }
}

impl<TSocket: AsyncRead + AsyncWrite> OutboundUpgrade<TSocket> for Rendezvous {
    type Output = Framed<TSocket, RendezvousCodec>;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        debug!("upgrading outbound");
        future::ready(Ok(Framed::new(socket, RendezvousCodec::default())))
    }
}
