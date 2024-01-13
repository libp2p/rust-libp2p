#![doc = include_str!("../README.md")]

use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};

use behaviour::Shared;
use futures::{
    channel::{mpsc, oneshot},
    SinkExt as _, StreamExt as _,
};
use handler::NewStream;
use libp2p_identity::PeerId;
use libp2p_swarm::{Stream, StreamProtocol};

mod behaviour;
mod handler;
mod upgrade;

pub use behaviour::{AlreadyRegistered, Behaviour};

/// A (remote) control for opening new streams for a particular protocol.
#[derive(Clone)]
pub struct Control {
    shared: Arc<Mutex<Shared>>,
}

impl Control {
    /// Obtain a [`PeerControl`] for the given [`PeerId`].
    ///
    /// This function will block until we have a connection to the given peer.
    pub async fn open_stream(
        &mut self,
        peer: PeerId,
        protocol: StreamProtocol,
    ) -> Result<Stream, OpenStreamError> {
        tracing::debug!(%peer, "Requesting new stream");

        let mut new_stream_sender = self.shared.lock().unwrap().sender(peer);

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
}

/// Errors while opening a new stream.
#[derive(Debug)]
pub enum OpenStreamError {
    /// The remote does not support the requested protocol.
    UnsupportedProtocol,
    /// IO Error that occurred during the protocol handshake.
    Io(std::io::Error),
}

impl From<std::io::Error> for OpenStreamError {
    fn from(v: std::io::Error) -> Self {
        Self::Io(v)
    }
}

/// A handle to inbound streams for a particular protocol.
#[must_use = "Streams do nothing unless polled."]
pub struct IncomingStreams {
    receiver: mpsc::Receiver<(PeerId, Stream)>,
}

impl futures::Stream for IncomingStreams {
    type Item = (PeerId, Stream);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_next_unpin(cx)
    }
}
