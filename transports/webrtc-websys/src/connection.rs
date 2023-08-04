//! Websys WebRTC Peer Connection
//!
use crate::stream::DataChannel;

use super::cbfutures::CbFuture;
use super::{Error, WebRTCStream};
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
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

    /// Peer Connection Getter
    pub(crate) fn peer_connection(&self) -> &RtcPeerConnection {
        &self.inner.peer_connection
    }
}
struct ConnectionInner {
    peer_connection: RtcPeerConnection,
    create_data_channel_cbfuture: CbFuture<RtcDataChannel>,
    closed: bool,
    ondatachannel_fut: CbFuture<RtcDataChannel>,

    /// A list of futures, which, once completed, signal that a [`WebRTCStream`] has been dropped.
    drop_listeners: FuturesUnordered<super::stream::DropListener>,
    no_drop_listeners_waker: Option<Waker>,
}

impl ConnectionInner {
    fn new(peer_connection: RtcPeerConnection) -> Self {
        // An ondatachannel Future enables us to poll for incoming data channel events in poll_incoming
        let ondatachannel_fut = CbFuture::new();
        let cback_clone = ondatachannel_fut.clone();

        // Wake the Future in the ondatachannel callback
        let ondatachannel_callback =
            Closure::<dyn FnMut(_)>::new(move |ev: RtcDataChannelEvent| {
                let dc2 = ev.channel();
                log::debug!("ondatachannel! Label (if any): {:?}", dc2.label());

                cback_clone.publish(dc2);
            });

        peer_connection.set_ondatachannel(Some(ondatachannel_callback.as_ref().unchecked_ref()));

        Self {
            peer_connection,
            create_data_channel_cbfuture: CbFuture::new(),
            closed: false,
            drop_listeners: FuturesUnordered::default(),
            no_drop_listeners_waker: None,
            ondatachannel_fut,
        }
    }

    /// Initiates and polls a future from `create_data_channel`.
    /// Takes the RtcPeerConnection and creates a regular DataChannel
    fn poll_create_data_channel(&mut self, cx: &mut Context) -> Poll<Result<WebRTCStream, Error>> {
        // Create Regular Data Channel
        let dc = DataChannel::new_regular(&self.peer_connection);
        let channel = WebRTCStream::new(dc);
        Poll::Ready(Ok(channel))
    }

    /// Polls the ondatachannel callback for inbound data channel stream.
    ///
    /// To poll for inbound WebRTCStreams, we need to poll for the ondatachannel callback
    /// We only get that callback for inbound data channels on our connections.
    /// This callback is converted to a future using CbFuture, which we can poll here
    fn poll_ondatachannel(&mut self, cx: &mut Context) -> Poll<Result<WebRTCStream, Error>> {
        // Poll the ondatachannel callback for incoming data channels
        let dc = ready!(self.ondatachannel_fut.poll_unpin(cx));

        // Create a WebRTCStream from the Data Channel
        let channel = WebRTCStream::new(DataChannel::Regular(dc));
        Poll::Ready(Ok(channel))
    }

    /// Poll the Inner Connection for Dropped Listeners
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
    type Substream = WebRTCStream; // A Substream of a WebRTC PeerConnection is a Data Channel
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
        log::debug!("connection::poll_close");

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
