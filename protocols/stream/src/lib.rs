#![doc = include_str!("../README.md")]

use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use behaviour::NewPeerControl;
use flume::r#async::SendSink;
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
// TODO: On `Drop`, we need to de-register the protocol.
#[derive(Clone)]
pub struct Control {
    protocol: StreamProtocol,
    sender: mpsc::Sender<NewPeerControl>,
}

impl Control {
    /// Obtain a [`PeerControl`] for the given [`PeerId`].
    ///
    /// This function will block until we have a connection to the given peer.
    pub async fn peer(&mut self, peer: PeerId) -> io::Result<PeerControl> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(NewPeerControl { peer, sender })
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionReset, e))?;
        let new_stream_sink = receiver
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::ConnectionReset, e))??;

        Ok(PeerControl {
            protocol: self.protocol.clone(),
            sender: new_stream_sink,
        })
    }
}

/// TODO: Keep connection alive whilst `PeerControl` is active.
#[derive(Clone)]
pub struct PeerControl {
    protocol: StreamProtocol,
    sender: SendSink<'static, NewStream>,
}

impl PeerControl {
    /// Open a new stream.
    pub async fn open_stream(&mut self) -> Result<Stream, OpenStreamError> {
        let (sender, receiver) = oneshot::channel();

        self.sender
            .send(NewStream {
                protocol: self.protocol.clone(),
                sender,
            })
            .await
            .map_err(|e| OpenStreamError::Io(io::Error::new(io::ErrorKind::BrokenPipe, e)))?;

        let stream = receiver
            .await
            .map_err(|e| OpenStreamError::Io(io::Error::new(io::ErrorKind::BrokenPipe, e)))??;

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
