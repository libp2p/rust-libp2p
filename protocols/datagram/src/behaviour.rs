use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Stream, StreamExt, channel::mpsc};
use libp2p_core::{Endpoint, Multiaddr, transport::PortUse};
use libp2p_identity::PeerId;
use libp2p_swarm::{
    ConnectionClosed, ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, NotifyHandler,
    StreamProtocol, THandler, THandlerInEvent, THandlerOutEvent, ToSwarm,
};

use crate::{Control, handler::Handler};

const CHANNEL_BUFFER: usize = 256;

/// Per-connection max outbound datagram size, shared with every [`Control`].
pub(crate) type DatagramSizes = Arc<Mutex<HashMap<(PeerId, ConnectionId), usize>>>;

/// Sends and receives unreliable datagrams for a single application protocol.
pub struct Behaviour {
    protocol: StreamProtocol,
    outbound_tx: mpsc::Sender<OutboundDatagram>,
    outbound_rx: mpsc::Receiver<OutboundDatagram>,
    inbound_tx: mpsc::Sender<(PeerId, Bytes)>,
    incoming: Option<mpsc::Receiver<(PeerId, Bytes)>>,
    sizes: DatagramSizes,
}

pub(crate) struct OutboundDatagram {
    pub(crate) peer: PeerId,
    pub(crate) connection: Option<ConnectionId>,
    pub(crate) data: Bytes,
}

impl Behaviour {
    /// Datagrams negotiated under `protocol` on the `/dg/1` control stream.
    pub fn new(protocol: StreamProtocol) -> Self {
        let (outbound_tx, outbound_rx) = mpsc::channel(CHANNEL_BUFFER);
        let (inbound_tx, incoming) = mpsc::channel(CHANNEL_BUFFER);
        Self {
            protocol,
            outbound_tx,
            outbound_rx,
            inbound_tx,
            incoming: Some(incoming),
            sizes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// A new [`Control`] for sending datagrams.
    pub fn new_control(&self) -> Control {
        Control::new(self.outbound_tx.clone(), self.sizes.clone())
    }

    /// Stream of inbound `(sender, payload)`. `None` after the first call.
    pub fn incoming_datagrams(&mut self) -> Option<IncomingDatagrams> {
        self.incoming.take().map(IncomingDatagrams)
    }
}

/// Stream of inbound datagrams tagged with the sending [`PeerId`].
pub struct IncomingDatagrams(mpsc::Receiver<(PeerId, Bytes)>);

impl Stream for IncomingDatagrams {
    type Item = (PeerId, Bytes);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_next_unpin(cx)
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = Handler;
    type ToSwarm = ();

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(
            peer,
            Endpoint::Listener,
            self.protocol.clone(),
            self.inbound_tx.clone(),
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        peer: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(Handler::new(
            peer,
            Endpoint::Dialer,
            self.protocol.clone(),
            self.inbound_tx.clone(),
        ))
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        if let FromSwarm::ConnectionClosed(ConnectionClosed {
            peer_id,
            connection_id,
            ..
        }) = event
        {
            self.sizes.lock().unwrap().remove(&(peer_id, connection_id));
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer: PeerId,
        connection: ConnectionId,
        max: THandlerOutEvent<Self>,
    ) {
        self.sizes.lock().unwrap().insert((peer, connection), max);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Poll::Ready(Some(out)) = self.outbound_rx.poll_next_unpin(cx) {
            return Poll::Ready(ToSwarm::NotifyHandler {
                peer_id: out.peer,
                handler: match out.connection {
                    Some(id) => NotifyHandler::One(id),
                    None => NotifyHandler::Any,
                },
                event: out.data,
            });
        }
        Poll::Pending
    }
}
