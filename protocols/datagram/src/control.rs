use bytes::Bytes;
use futures::channel::mpsc;
use libp2p_identity::PeerId;
use libp2p_swarm::ConnectionId;

use crate::behaviour::{DatagramSizes, OutboundDatagram};

/// Cloneable handle for sending datagrams.
#[derive(Clone)]
pub struct Control {
    sender: mpsc::Sender<OutboundDatagram>,
    sizes: DatagramSizes,
}

impl Control {
    pub(crate) fn new(sender: mpsc::Sender<OutboundDatagram>, sizes: DatagramSizes) -> Self {
        Self { sender, sizes }
    }

    /// The connection's current max outbound datagram size, learned from sends
    /// on it. `None` until the first datagram has been sent.
    pub fn max_datagram_size(&self, peer: PeerId, connection: ConnectionId) -> Option<usize> {
        self.sizes.lock().unwrap().get(&(peer, connection)).copied()
    }

    /// Send to `peer` over whichever connection the swarm picks.
    pub fn send_datagram(&mut self, peer: PeerId, data: Bytes) -> Result<(), SendError> {
        self.enqueue(peer, None, data)
    }

    /// Send pinned to a specific `connection`.
    pub fn send_datagram_on_connection(
        &mut self,
        peer: PeerId,
        connection: ConnectionId,
        data: Bytes,
    ) -> Result<(), SendError> {
        self.enqueue(peer, Some(connection), data)
    }

    fn enqueue(
        &mut self,
        peer: PeerId,
        connection: Option<ConnectionId>,
        data: Bytes,
    ) -> Result<(), SendError> {
        self.sender
            .try_send(OutboundDatagram {
                peer,
                connection,
                data,
            })
            .map_err(|e| {
                if e.is_full() {
                    SendError::Full
                } else {
                    SendError::Closed
                }
            })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("datagram queue is full; datagram dropped")]
    Full,
    #[error("datagram behaviour has shut down")]
    Closed,
}
