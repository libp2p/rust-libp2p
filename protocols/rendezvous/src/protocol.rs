use asynchronous_codec::{BytesMut, Decoder, Encoder, Framed};
use libp2p_core::{InboundUpgrade, OutboundUpgrade, UpgradeInfo};
use std::{future, iter};
use void::Void;
use futures::AsyncRead;
use futures::AsyncWrite;
use crate::codec::RendezvousCodec;
use crate::codec;

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

impl<TSocket: AsyncRead + AsyncWrite> InboundUpgrade<TSocket> for Rendezvous {
    type Output = InboundStream<TSocket>;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        future::ready(todo!())
    }
}

impl<TSocket: AsyncRead + AsyncWrite> OutboundUpgrade<TSocket> for Rendezvous {
    //type Output = Framed<TSocket, RendezvousCodec>;
    type Output = DiscoverResponse;
    type Error = Void;
    type Future = future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, socket: TSocket, _: Self::Info) -> Self::Future {
        //future::ready(Ok(Framed::new(socket, RendezvousCodec)))
        future::ready(todo!())
    }
}

pub enum InboundStream<TSocket> {
    Register(RegisterRequest<TSocket>),
    Discover
}

pub enum Outbound<TSocket> {
    Register(RegisterRequest<TSocket>),
    Discover
}

pub struct RegisterRequest<TSocket> {

}

impl<TSocket> RegisterRequest<TSocket> {
    pub fn respond_with(self, response: RegisterResponse) -> RegisterResponseFuture<TSocket> {
        todo!()
    }
}

pub struct DiscoverRequest<TSocket> {

}

pub enum DiscoverResponse {
    Registrations,
    Error,
}

pub enum RegisterResponse {
    Ok,
    Error
}

pub struct RegisterResponseFuture<TSocket> {
    stream: Framed<TSocket, RendezvousCodec>,
    message: codec::Message
}
