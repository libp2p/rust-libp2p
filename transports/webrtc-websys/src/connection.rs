//! Websys WebRTC Peer Connection
//!
use crate::stream::RtcDataChannelBuilder;

use super::{Error, Stream};
use futures::channel;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use send_wrapper::SendWrapper;
use std::pin::Pin;
use std::task::Waker;
use std::task::{ready, Context, Poll};
use wasm_bindgen::prelude::*;
use web_sys::{RtcDataChannel, RtcDataChannelEvent, RtcPeerConnection};

pub struct Connection {
    // Swarm needs all types to be Send. WASM is single-threaded
    // and it is safe to use SendWrapper.
    inner: SendWrapper<ConnectionInner>,
}

impl Connection {
    /// Create a new Connection
    pub(crate) fn new(peer_connection: RtcPeerConnection) -> Self {
        Self {
            inner: SendWrapper::new(ConnectionInner::new(peer_connection)),
        }
    }
}
struct ConnectionInner {
    peer_connection: RtcPeerConnection,
    closed: bool,
    rx_ondatachannel: channel::mpsc::Receiver<RtcDataChannel>,

    /// A list of futures, which, once completed, signal that a [`WebRTCStream`] has been dropped.
    drop_listeners: FuturesUnordered<super::stream::DropListener>,
    no_drop_listeners_waker: Option<Waker>,
}

impl ConnectionInner {
    fn new(peer_connection: RtcPeerConnection) -> Self {
        // An ondatachannel Future enables us to poll for incoming data channel events in poll_incoming
        let (mut tx_ondatachannel, rx_ondatachannel) = channel::mpsc::channel(4); // we may get more than one data channel opened on a single peer connection

        // Wake the Future in the ondatachannel callback
        let ondatachannel_callback =
            Closure::<dyn FnMut(_)>::new(move |ev: RtcDataChannelEvent| {
                let dc2 = ev.channel();
                log::trace!("ondatachannel_callback triggered");
                match tx_ondatachannel.try_send(dc2) {
                    Ok(_) => log::trace!("ondatachannel_callback sent data channel"),
                    Err(e) => log::error!(
                        "ondatachannel_callback: failed to send data channel: {:?}",
                        e
                    ),
                }
            });

        peer_connection.set_ondatachannel(Some(ondatachannel_callback.as_ref().unchecked_ref()));
        ondatachannel_callback.forget();

        Self {
            peer_connection,
            closed: false,
            drop_listeners: FuturesUnordered::default(),
            no_drop_listeners_waker: None,
            rx_ondatachannel,
        }
    }

    /// Initiates and polls a future from `create_data_channel`.
    /// Takes the RtcPeerConnection and creates a regular DataChannel
    fn poll_create_data_channel(&mut self, _cx: &mut Context) -> Poll<Result<Stream, Error>> {
        // Create Regular Data Channel
        log::trace!("Creating outbound data channel");
        let dc = RtcDataChannelBuilder::default().build_with(&self.peer_connection);
        let channel = Stream::new(dc);
        Poll::Ready(Ok(channel))
    }

    /// Polls the ondatachannel callback for inbound data channel stream.
    ///
    /// To poll for inbound WebRTCStreams, we need to poll for the ondatachannel callback
    /// We only get that callback for inbound data channels on our connections.
    /// This callback is converted to a future using channel, which we can poll here
    fn poll_ondatachannel(&mut self, cx: &mut Context) -> Poll<Result<Stream, Error>> {
        match ready!(self.rx_ondatachannel.poll_next_unpin(cx)) {
            Some(dc) => {
                // Create a WebRTC Stream from the Data Channel
                let channel = Stream::new(dc);
                log::trace!("connection::poll_ondatachannel ready");
                Poll::Ready(Ok(channel))
            }
            None => {
                self.no_drop_listeners_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    /// Poll the Connection
    ///
    /// Mostly handles Dropped Listeners
    fn poll(&mut self, cx: &mut Context) -> Poll<Result<StreamMuxerEvent, Error>> {
        loop {
            match ready!(self.drop_listeners.poll_next_unpin(cx)) {
                Some(Ok(())) => {}
                Some(Err(e)) => {
                    log::debug!("a DropListener failed: {e}")
                }
                None => {
                    self.no_drop_listeners_waker = Some(cx.waker().clone());
                    return Poll::Pending;
                }
            }
        }
    }

    /// Closes the Peer Connection.
    ///
    /// This closes the data channels also and they will return an error
    /// if they are used.
    fn close_connection(&mut self) {
        if !self.closed {
            log::trace!("connection::close_connection");
            self.peer_connection.close();
            self.closed = true;
        }
    }
}

impl Drop for ConnectionInner {
    fn drop(&mut self) {
        self.close_connection();
    }
}

/// WebRTC native multiplexing
/// Allows users to open substreams
impl StreamMuxer for Connection {
    type Substream = Stream; // A Substream of a WebRTC PeerConnection is a Data Channel
    type Error = Error;

    fn poll_inbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        // Inbound substream is signalled by an ondatachannel event
        self.inner.poll_ondatachannel(cx)
    }

    // We create the Data Channel here from the Peer Connection
    // then wait for the Data Channel to be opened
    fn poll_outbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        self.inner.poll_create_data_channel(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        log::trace!("connection::poll_close");

        self.inner.close_connection();
        Poll::Ready(Ok(()))
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
        self.inner.poll(cx)
    }
}
