use asynchronous_codec::{BytesMut, Decoder, Encoder, Framed};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use libp2p_swarm::NegotiatedSubstream;
use std::io::Error;
use std::{future, iter};
use void::Void;

#[derive(Default, Debug, Copy, Clone)]
pub struct Rendezvous;

impl Rendezvous {
    pub fn new() -> Rendezvous {
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

impl<TSocket> InboundUpgrade<TSocket> for Rendezvous {
    type Output = Framed<TSocket, RendezvousCodec>;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: TSocket, protocol_id: Self::Info) -> Self::Future {
        future::ok(Framed::new(socket, RendezvousCodec))
    }
}

impl<TSocket> OutboundUpgrade<TSocket> for Rendezvous {
    type Output = Framed<TSocket, RendezvousCodec>;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: TSocket, protocol_id: Self::Info) -> Self::Future {
        future::ok(Framed::new(socket, RendezvousCodec))
    }
}

/// The event emitted by the Handler. This informs the behaviour of various events created
/// by the handler.
#[derive(Debug)]
pub enum Message {
    RegisterReq,
    DiscoverReq,
    RegisterResp,
    DiscoverResp,
}

struct RendezvousCodec;

impl Encoder for RendezvousCodec {
    type Item = Message;
    type Error = std::io::Error;
}

impl Decoder for RendezvousCodec {
    type Item = Message;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        todo!("decode src and return it")
    }
}

mod wire {
    include!(concat!(env!("OUT_DIR"), "/rendezvous.pb.rs"));
}
