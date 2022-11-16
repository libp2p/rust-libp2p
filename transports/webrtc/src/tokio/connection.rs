// Copyright 2022 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use futures::stream::FuturesUnordered;
use futures::{
    channel::{
        mpsc,
        oneshot::{self, Sender},
    },
    lock::Mutex as FutMutex,
    StreamExt,
    {future::BoxFuture, ready},
};
use libp2p_core::muxing::{StreamMuxer, StreamMuxerEvent};
use webrtc::data::data_channel::DataChannel as DetachedDataChannel;
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::RTCPeerConnection;

use std::task::Waker;
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use crate::tokio::{error::Error, substream, substream::Substream};

/// Maximum number of unprocessed data channels.
/// See [`Connection::poll_inbound`].
const MAX_DATA_CHANNELS_IN_FLIGHT: usize = 10;

/// A WebRTC connection, wrapping [`RTCPeerConnection`] and implementing [`StreamMuxer`] trait.
pub struct Connection {
    /// [`RTCPeerConnection`] to the remote peer.
    ///
    /// Uses futures mutex because used in async code (see poll_outbound and poll_close).
    peer_conn: Arc<FutMutex<RTCPeerConnection>>,

    /// Channel onto which incoming data channels are put.
    incoming_data_channels_rx: mpsc::Receiver<Arc<DetachedDataChannel>>,

    /// Future, which, once polled, will result in an outbound substream.
    outbound_fut: Option<BoxFuture<'static, Result<Arc<DetachedDataChannel>, Error>>>,

    /// Future, which, once polled, will result in closing the entire connection.
    close_fut: Option<BoxFuture<'static, Result<(), Error>>>,

    /// A list of futures, which, once completed, signal that a [`Substream`] has been dropped.
    drop_listeners: FuturesUnordered<substream::DropListener>,
    no_drop_listeners_waker: Option<Waker>,
}

impl Unpin for Connection {}

impl Connection {
    /// Creates a new connection.
    pub(crate) async fn new(rtc_conn: RTCPeerConnection) -> Self {
        let (data_channel_tx, data_channel_rx) = mpsc::channel(MAX_DATA_CHANNELS_IN_FLIGHT);

        Connection::register_incoming_data_channels_handler(
            &rtc_conn,
            Arc::new(FutMutex::new(data_channel_tx)),
        )
        .await;

        Self {
            peer_conn: Arc::new(FutMutex::new(rtc_conn)),
            incoming_data_channels_rx: data_channel_rx,
            outbound_fut: None,
            close_fut: None,
            drop_listeners: FuturesUnordered::default(),
            no_drop_listeners_waker: None,
        }
    }

    /// Registers a handler for incoming data channels.
    ///
    /// NOTE: `mpsc::Sender` is wrapped in `Arc` because cloning a raw sender would make the channel
    /// unbounded. "The channel’s capacity is equal to buffer + num-senders. In other words, each
    /// sender gets a guaranteed slot in the channel capacity..."
    /// See <https://docs.rs/futures/latest/futures/channel/mpsc/fn.channel.html>
    async fn register_incoming_data_channels_handler(
        rtc_conn: &RTCPeerConnection,
        tx: Arc<FutMutex<mpsc::Sender<Arc<DetachedDataChannel>>>>,
    ) {
        rtc_conn.on_data_channel(Box::new(move |data_channel: Arc<RTCDataChannel>| {
            log::debug!("Incoming data channel {}", data_channel.id());

            let tx = tx.clone();

            Box::pin(async move {
                data_channel.on_open({
                    let data_channel = data_channel.clone();
                    Box::new(move || {
                        log::debug!("Data channel {} open", data_channel.id());

                        Box::pin(async move {
                            let data_channel = data_channel.clone();
                            let id = data_channel.id();
                            match data_channel.detach().await {
                                Ok(detached) => {
                                    let mut tx = tx.lock().await;
                                    if let Err(e) = tx.try_send(detached.clone()) {
                                        log::error!("Can't send data channel {}: {}", id, e);
                                        // We're not accepting data channels fast enough =>
                                        // close this channel.
                                        //
                                        // Ideally we'd refuse to accept a data channel
                                        // during the negotiation process, but it's not
                                        // possible with the current API.
                                        if let Err(e) = detached.close().await {
                                            log::error!(
                                                "Failed to close data channel {}: {}",
                                                id,
                                                e
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::error!("Can't detach data channel {}: {}", id, e);
                                }
                            };
                        })
                    })
                });
            })
        }));
    }
}

impl StreamMuxer for Connection {
    type Substream = Substream;
    type Error = Error;

    fn poll_inbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        match ready!(self.incoming_data_channels_rx.poll_next_unpin(cx)) {
            Some(detached) => {
                log::trace!("Incoming substream {}", detached.stream_identifier());

                let (substream, drop_listener) = Substream::new(detached);
                self.drop_listeners.push(drop_listener);
                if let Some(waker) = self.no_drop_listeners_waker.take() {
                    waker.wake()
                }

                Poll::Ready(Ok(substream))
            }
            None => {
                debug_assert!(
                    false,
                    "Sender-end of channel should be owned by `RTCPeerConnection`"
                );

                Poll::Pending // Return `Pending` without registering a waker: If the channel is closed, we don't need to be called anymore.
            }
        }
    }

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>> {
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

    fn poll_outbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let peer_conn = self.peer_conn.clone();
        let fut = self.outbound_fut.get_or_insert(Box::pin(async move {
            let peer_conn = peer_conn.lock().await;

            let data_channel = peer_conn.create_data_channel("", None).await?;

            // No need to hold the lock during the DTLS handshake.
            drop(peer_conn);

            log::trace!("Opening data channel {}", data_channel.id());

            let (tx, rx) = oneshot::channel::<Arc<DetachedDataChannel>>();

            // Wait until the data channel is opened and detach it.
            register_data_channel_open_handler(data_channel, tx).await;

            // Wait until data channel is opened and ready to use
            match rx.await {
                Ok(detached) => Ok(detached),
                Err(e) => Err(Error::Internal(e.to_string())),
            }
        }));

        match ready!(fut.as_mut().poll(cx)) {
            Ok(detached) => {
                self.outbound_fut = None;

                log::trace!("Outbound substream {}", detached.stream_identifier());

                let (substream, drop_listener) = Substream::new(detached);
                self.drop_listeners.push(drop_listener);
                if let Some(waker) = self.no_drop_listeners_waker.take() {
                    waker.wake()
                }

                Poll::Ready(Ok(substream))
            }
            Err(e) => {
                self.outbound_fut = None;
                Poll::Ready(Err(e))
            }
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        log::debug!("Closing connection");

        let peer_conn = self.peer_conn.clone();
        let fut = self.close_fut.get_or_insert(Box::pin(async move {
            let peer_conn = peer_conn.lock().await;
            peer_conn.close().await?;

            Ok(())
        }));

        match ready!(fut.as_mut().poll(cx)) {
            Ok(()) => {
                self.incoming_data_channels_rx.close();
                self.close_fut = None;
                Poll::Ready(Ok(()))
            }
            Err(e) => {
                self.close_fut = None;
                Poll::Ready(Err(e))
            }
        }
    }
}

pub(crate) async fn register_data_channel_open_handler(
    data_channel: Arc<RTCDataChannel>,
    data_channel_tx: Sender<Arc<DetachedDataChannel>>,
) {
    data_channel.on_open({
        let data_channel = data_channel.clone();
        Box::new(move || {
            log::debug!("Data channel {} open", data_channel.id());

            Box::pin(async move {
                let data_channel = data_channel.clone();
                let id = data_channel.id();
                match data_channel.detach().await {
                    Ok(detached) => {
                        if let Err(e) = data_channel_tx.send(detached.clone()) {
                            log::error!("Can't send data channel {}: {:?}", id, e);
                            if let Err(e) = detached.close().await {
                                log::error!("Failed to close data channel {}: {}", id, e);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("Can't detach data channel {}: {}", id, e);
                    }
                };
            })
        })
    });
}
