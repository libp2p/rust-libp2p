use std::{
    io,
    task::{Context, Poll},
};

use behaviour::NewPeerControl;
use flume::r#async::SendSink;
use futures::{
    channel::{mpsc, oneshot},
    SinkExt as _, StreamExt as _,
};
use handler::NewStream;
use libp2p::{PeerId, Stream, StreamProtocol};

mod behaviour;
mod handler;
mod upgrade;

pub use behaviour::{AlreadyRegistered, Behaviour};

// TODO: On `Drop`, we need to de-register the protocol.
#[derive(Clone)]
pub struct Control {
    protocol: StreamProtocol,
    sender: mpsc::Sender<NewPeerControl>,
}

impl Control {
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

pub struct PeerControl {
    protocol: StreamProtocol,
    sender: SendSink<'static, NewStream>,
}

impl PeerControl {
    // TOOD: Things that we could make configurable:
    // - Timeout for opening a new stream
    pub async fn open_stream(&mut self) -> Result<Stream, Error> {
        let (sender, receiver) = oneshot::channel();

        self.sender
            .send(NewStream {
                protocol: self.protocol.clone(),
                sender,
            })
            .await
            .map_err(|e| Error::Io(io::Error::new(io::ErrorKind::BrokenPipe, e)))?;

        let stream = receiver
            .await
            .map_err(|e| Error::Io(io::Error::new(io::ErrorKind::BrokenPipe, e)))??;

        Ok(stream)
    }
}

#[derive(Debug)]
pub enum Error {
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

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        self.receiver.poll_next_unpin(cx)
    }
}
