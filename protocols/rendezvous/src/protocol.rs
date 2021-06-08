use crate::codec::RendezvousCodec;
use asynchronous_codec::Framed;
use libp2p_core::upgrade::FromFnUpgrade;
use libp2p_core::{upgrade, Endpoint};
use libp2p_swarm::NegotiatedSubstream;
use std::future;
use upgrade::from_fn;
use void::Void;

pub fn new() -> Rendezvous {
    from_fn(
        b"/rendezvous/1.0.0",
        Box::new(|socket, _| future::ready(Ok(Framed::new(socket, RendezvousCodec::default())))),
    )
}

pub type Rendezvous = FromFnUpgrade<
    &'static [u8],
    Box<
        dyn Fn(
                NegotiatedSubstream,
                Endpoint,
            )
                -> future::Ready<Result<Framed<NegotiatedSubstream, RendezvousCodec>, Void>>
            + Send
            + 'static,
    >,
>;
