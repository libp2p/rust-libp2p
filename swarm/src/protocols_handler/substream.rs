use crate::protocols_handler::{InboundUpgradeSend, OutboundUpgradeSend};
use crate::{
    KeepAlive, NegotiatedSubstream, ProtocolsHandler, ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr, SubstreamProtocol,
};
use futures::future::BoxFuture;
use futures::{future, FutureExt};
use libp2p_core::upgrade::FromFnUpgrade;
use libp2p_core::Endpoint;
use std::collections::{HashMap, VecDeque};
use std::task::{Context, Poll};
use void::Void;

// TODO: Should a substream be able to close the entire connection?
pub trait SubstreamHandler {
    type InEvent;
    type OutEvent;
    type OutboundOpenInfo;

    fn new_inbound(substream: NegotiatedSubstream) -> Self;
    fn new_outbound(substream: NegotiatedSubstream, open_info: Self::OutboundOpenInfo) -> Self;

    fn inject_event(&mut self, event: Self::InEvent);
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Option<Self::OutEvent>>; // None means done
}

pub struct OneshotPushSubstream<M> {
    future: BoxFuture<'static, Option<M>>,
    done: bool,
}

impl<M> SubstreamHandler for OneshotPushSubstream<M> {
    type InEvent = ();
    type OutEvent = Option<M>; // Some means inbound read a message, None means outbound sent a message
    type OutboundOpenInfo = M;

    fn new_inbound(mut substream: NegotiatedSubstream) -> Self {
        Self {
            future: async move {
                // let _result = libp2p_core::upgrade::read_one(&mut substream, 1024).await;

                let message = todo!("deserialize message");

                Some(message)
            }
            .boxed(),
            done: false,
        }
    }

    fn new_outbound(mut substream: NegotiatedSubstream, open_info: Self::OutboundOpenInfo) -> Self {
        Self {
            future: async move {
                // let bytes = todo!("serialize message");
                // libp2p_core::upgrade::write_one(&mut substream, bytes).await;

                None
            }
            .boxed(),
            done: false,
        }
    }

    fn inject_event(&mut self, _: Self::InEvent) {
        unreachable!()
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Option<Self::OutEvent>> {
        if self.done {
            // avoid poll-after-ready
            return Poll::Ready(None);
        }

        let message = futures::ready!(self.future.poll_unpin(cx));
        self.done = true;

        Poll::Ready(Some(message))
    }
}

#[derive(Debug, Hash, Eq, PartialEq)]
pub struct SubstreamId(u64);

type ProtocolUpgradeFn = Box<
    dyn Fn(NegotiatedSubstream, Endpoint) -> future::Ready<Result<NegotiatedSubstream, Void>>
        + Send,
>;
type Protocol = FromFnUpgrade<&'static [u8], ProtocolUpgradeFn>;

pub struct ProtocolsHandlerAdaptor<S, TOutboundOpenInfo> {
    substreams: HashMap<SubstreamId, S>,
    protocol: &'static [u8],
    next_substream_id: u64,

    new_substreams: VecDeque<TOutboundOpenInfo>,
}

impl<S, TOutboundOpenInfo> ProtocolsHandlerAdaptor<S, TOutboundOpenInfo> {
    pub fn new(protocol: &'static [u8]) -> Self {
        Self {
            substreams: Default::default(),
            protocol,
            next_substream_id: 0,
            new_substreams: Default::default(),
        }
    }
}

pub enum InEvent<I, E> {
    NewSubstream { open_info: I },
    NotifySubstream { substream: SubstreamId, message: E },
}

pub enum OutEvent<O> {
    Message { substream: SubstreamId, message: O },
}

impl<TInEvent, TOutEvent, TOutboundOpenInfo, TSubstreamHandler> ProtocolsHandler
    for ProtocolsHandlerAdaptor<TSubstreamHandler, TOutboundOpenInfo>
where
    TSubstreamHandler: SubstreamHandler<
        InEvent = TInEvent,
        OutEvent = TOutEvent,
        OutboundOpenInfo = TOutboundOpenInfo,
    >,
    TInEvent: Send + 'static,
    TOutEvent: Send + 'static,
    TOutboundOpenInfo: Send + 'static,
    TSubstreamHandler: Send + 'static,
{
    type InEvent = InEvent<TOutboundOpenInfo, TInEvent>;
    type OutEvent = OutEvent<TOutEvent>;
    type Error = Void;
    type InboundProtocol = Protocol;
    type OutboundProtocol = Protocol;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = TOutboundOpenInfo;

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        SubstreamProtocol::new(
            libp2p_core::upgrade::from_fn(
                self.protocol.clone(),
                Box::new(|socket, _| future::ready(Ok(socket))),
            ),
            (),
        )
    }

    fn inject_fully_negotiated_inbound(
        &mut self,
        protocol: <Self::InboundProtocol as InboundUpgradeSend>::Output,
        _: Self::InboundOpenInfo,
    ) {
        let id = SubstreamId(self.next_substream_id);
        self.next_substream_id += 1;

        self.substreams
            .insert(id, TSubstreamHandler::new_inbound(protocol));
    }

    fn inject_fully_negotiated_outbound(
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        info: Self::OutboundOpenInfo,
    ) {
        let id = SubstreamId(self.next_substream_id);
        self.next_substream_id += 1;

        self.substreams
            .insert(id, TSubstreamHandler::new_outbound(protocol, info));
    }

    fn inject_event(&mut self, event: Self::InEvent) {
        match event {
            InEvent::NewSubstream { open_info } => self.new_substreams.push_back(open_info),
            InEvent::NotifySubstream { substream, message } => {
                // find substream in hash maps
                // call `inject_event` on substream (for this we need to define a trait for the state)
            }
        }
    }

    fn inject_dial_upgrade_error(
        &mut self,
        _: Self::OutboundOpenInfo,
        _: ProtocolsHandlerUpgrErr<Void>,
    ) {
        unreachable!("our upgrade can't fail")
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        // TODO: Allow substreams to control keep alive somehow?

        KeepAlive::Yes
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<
        ProtocolsHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::OutEvent,
            Self::Error,
        >,
    > {
        if let Some(open_info) = self.new_substreams.pop_front() {
            return Poll::Ready(ProtocolsHandlerEvent::OutboundSubstreamRequest {
                protocol: SubstreamProtocol::new(
                    libp2p_core::upgrade::from_fn(
                        self.protocol.clone(),
                        Box::new(|socket, _| future::ready(Ok(socket))),
                    ),
                    open_info,
                ),
            });
        }

        // TODO poll all inbound streams
        // TODO poll all outbound streams

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NetworkBehaviour, NetworkBehaviourAction, NotifyHandler, PollParameters};
    use libp2p_core::connection::ConnectionId;
    use libp2p_core::{Multiaddr, PeerId};
    use std::task::{Context, Poll};

    pub enum State {
        Inbound { substream: NegotiatedSubstream },
        Outbound { substream: NegotiatedSubstream },
    }

    pub struct PingHandler {
        state: State,
    }

    impl SubstreamHandler for PingHandler {
        type InEvent = ();
        type OutEvent = ();
        type OutboundOpenInfo = ();

        fn new_inbound(substream: NegotiatedSubstream) -> Self {
            Self {
                state: State::Inbound { substream },
            }
        }

        fn new_outbound(substream: NegotiatedSubstream, _: Self::OutboundOpenInfo) -> Self {
            Self {
                state: State::Inbound { substream },
            }
        }

        fn inject_event(&mut self, _: Self::InEvent) {
            unreachable!("no events in ping protocol")
        }

        fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Option<Self::OutEvent>> {
            // implement protocol logic here
            // poll substreams as necessary
            // return `None` once done

            Poll::Pending
        }
    }

    pub struct Behaviour {}

    impl NetworkBehaviour for Behaviour {
        type ProtocolsHandler = ProtocolsHandlerAdaptor<PingHandler, ()>;
        type OutEvent = ();

        fn new_handler(&mut self) -> Self::ProtocolsHandler {
            todo!()
        }

        fn addresses_of_peer(&mut self, peer_id: &PeerId) -> Vec<Multiaddr> {
            todo!()
        }

        fn inject_connected(&mut self, peer_id: &PeerId) {
            todo!()
        }

        fn inject_disconnected(&mut self, peer_id: &PeerId) {
            todo!()
        }

        fn inject_event(&mut self, peer_id: PeerId, connection: ConnectionId, event: OutEvent<()>) {
            let substream_id = match event {
                OutEvent::Message { substream, message } => substream,
            };

            let unique_id_of_substream = (peer_id, connection, substream_id);

            // ------- how to open a new substream

            let make_new_substream = NetworkBehaviourAction::<_, ()>::NotifyHandler {
                peer_id,
                handler: NotifyHandler::Any,
                event: InEvent::<_, ()>::NewSubstream { open_info: () },
            };

            // ------- how to notify a specific substream

            let (peer_id, connection_id, substream_id) =
                todo!("grab 'unique-id' from a hashmap in self of some sorts");

            let notify_specific_substream = NetworkBehaviourAction::<_, ()>::NotifyHandler {
                peer_id,
                handler: NotifyHandler::One(connection_id),
                event: InEvent::<(), _>::NotifySubstream {
                    substream: substream_id,
                    message: (),
                },
            };
        }

        fn poll(
            &mut self,
            cx: &mut Context<'_>,
            params: &mut impl PollParameters,
        ) -> Poll<NetworkBehaviourAction<InEvent<(), ()>, Self::OutEvent>> {
            todo!()
        }
    }
}
