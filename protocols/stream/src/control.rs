use core::fmt;
use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use futures::{
    channel::{mpsc, oneshot},
    SinkExt as _, StreamExt as _,
};
use libp2p_identity::PeerId;
use libp2p_swarm::{Stream, StreamProtocol};

use crate::{handler::NewStream, shared::Shared, AlreadyRegistered};

/// A (remote) control for opening new streams and registration of inbound protocols.
///
/// A [`Control`] can be cloned and thus allows for concurrent access.
#[derive(Clone)]
pub struct Control {
    shared: Arc<Mutex<Shared>>,
}

impl Control {
    pub(crate) fn new(shared: Arc<Mutex<Shared>>) -> Self {
        Self { shared }
    }

    /// Attempt to open a new stream for the given protocol and peer.
    ///
    /// In case we are currently not connected to the peer,
    /// we will attempt to make a new connection.
    ///
    /// ## Backpressure
    ///
    /// [`Control`]s support backpressure similarly to bounded channels:
    /// Each [`Control`] has a guaranteed slot for internal messages.
    /// A single control will always open one stream at a
    /// time which is enforced by requiring `&mut self`.
    ///
    /// This backpressure mechanism breaks if you clone [`Control`]s excessively.
    pub async fn open_stream(
        &mut self,
        peer: PeerId,
        protocol: StreamProtocol,
    ) -> Result<Stream, OpenStreamError> {
        tracing::debug!(%peer, "Requesting new stream");

        let mut new_stream_sender = Shared::lock(&self.shared).sender(peer);

        let (sender, receiver) = oneshot::channel();

        new_stream_sender
            .send(NewStream { protocol, sender })
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionReset, e))?;

        let stream = receiver
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionReset, e))??;

        Ok(stream)
    }

    /// Accept inbound streams for the provided protocol.
    ///
    /// To stop accepting streams, simply drop the returned [`IncomingStreams`] handle.
    pub fn accept(
        &mut self,
        protocol: StreamProtocol,
    ) -> Result<IncomingStreams, AlreadyRegistered> {
        Shared::lock(&self.shared).accept(protocol)
    }
}

/// Errors while opening a new stream.
#[derive(Debug)]
#[non_exhaustive]
pub enum OpenStreamError {
    /// The remote does not support the requested protocol.
    UnsupportedProtocol(StreamProtocol),
    /// IO Error that occurred during the protocol handshake.
    Io(std::io::Error),
}

impl From<std::io::Error> for OpenStreamError {
    fn from(v: std::io::Error) -> Self {
        Self::Io(v)
    }
}

impl fmt::Display for OpenStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenStreamError::UnsupportedProtocol(p) => {
                write!(f, "failed to open stream: remote peer does not support {p}")
            }
            OpenStreamError::Io(e) => {
                write!(f, "failed to open stream: io error: {e}")
            }
        }
    }
}

impl std::error::Error for OpenStreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// A handle to inbound streams for a particular protocol.
#[must_use = "Streams do nothing unless polled."]
pub struct IncomingStreams {
    receiver: mpsc::Receiver<(PeerId, Stream)>,
}

impl IncomingStreams {
    pub(crate) fn new(receiver: mpsc::Receiver<(PeerId, Stream)>) -> Self {
        Self { receiver }
    }
}

impl futures::Stream for IncomingStreams {
    type Item = (PeerId, Stream);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_next_unpin(cx)
    }
}
