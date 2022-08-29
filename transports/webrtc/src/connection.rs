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

mod poll_data_channel;

use futures::{
    channel::{
        mpsc,
        oneshot::{self, Sender},
    },
    lock::Mutex as FutMutex,
    {future::BoxFuture, prelude::*, ready},
};
use futures_lite::StreamExt;
use libp2p_core::{muxing::StreamMuxer, Multiaddr};
use log::{debug, error, trace};
use webrtc::data::data_channel::DataChannel as DetachedDataChannel;
use webrtc::data_channel::RTCDataChannel;
use webrtc::peer_connection::RTCPeerConnection;

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use crate::error::Error;
pub(crate) use poll_data_channel::PollDataChannel;

const MAX_DATA_CHANNELS_IN_FLIGHT: usize = 10;

/// A WebRTC connection, wrapping [`RTCPeerConnection`] and implementing [`StreamMuxer`] trait.
pub struct Connection {
    /// `RTCPeerConnection` to the remote peer.
    ///
    /// Uses futures mutex because used in async code (see poll_outbound and poll_close).
    peer_conn: Arc<FutMutex<RTCPeerConnection>>,

    /// Channel onto which incoming data channels are put.
    incoming_data_channels_rx: mpsc::Receiver<Arc<DetachedDataChannel>>,

    /// Temporary read buffer's capacity (equal for all data channels).
    /// See [`PollDataChannel`] `read_buf_cap`.
    read_buf_cap: Option<usize>,

    /// Future, which, once polled, will result in an outbound substream.
    outbound_fut: Option<BoxFuture<'static, Result<Arc<DetachedDataChannel>, Error>>>,

    /// Future, which, once polled, will result in closing the entire connection.
    close_fut: Option<BoxFuture<'static, Result<(), Error>>>,
}

impl Unpin for Connection {}

impl Connection {
    /// Creates a new connection.
    pub async fn new(rtc_conn: RTCPeerConnection) -> Self {
        let (data_channel_tx, data_channel_rx) = mpsc::channel(MAX_DATA_CHANNELS_IN_FLIGHT);

        Connection::register_incoming_data_channels_handler(&rtc_conn, data_channel_tx).await;

        Self {
            peer_conn: Arc::new(FutMutex::new(rtc_conn)),
            incoming_data_channels_rx: data_channel_rx,
            read_buf_cap: None,
            outbound_fut: None,
            close_fut: None,
        }
    }

    /// Set the capacity of a data channel's temporary read buffer (equal for all data channels; default: 8192).
    pub fn set_data_channels_read_buf_capacity(&mut self, cap: usize) {
        self.read_buf_cap = Some(cap);
    }

    /// Registers a handler for incoming data channels.
    async fn register_incoming_data_channels_handler(
        rtc_conn: &RTCPeerConnection,
        tx: mpsc::Sender<Arc<DetachedDataChannel>>,
    ) {
        rtc_conn
            .on_data_channel(Box::new(move |data_channel: Arc<RTCDataChannel>| {
                debug!(
                    "Incoming data channel '{}'-'{}'",
                    data_channel.label(),
                    data_channel.id()
                );

                let data_channel = data_channel.clone();
                let mut tx = tx.clone();

                Box::pin(async move {
                    data_channel
                        .on_open({
                            let data_channel = data_channel.clone();
                            Box::new(move || {
                                debug!(
                                    "Data channel '{}'-'{}' open",
                                    data_channel.label(),
                                    data_channel.id()
                                );

                                Box::pin(async move {
                                    let data_channel = data_channel.clone();
                                    match data_channel.detach().await {
                                        Ok(detached) => {
                                            if let Err(e) = tx.try_send(detached.clone()) {
                                                error!("Can't send data channel: {}", e);
                                                // We're not accepting data channels fast enough =>
                                                // close this channel.
                                                //
                                                // Ideally we'd refuse to accept a data channel
                                                // during the negotiation process, but it's not
                                                // possible with the current API.
                                                if let Err(e) = detached.close().await {
                                                    error!("Failed to close data channel: {}", e);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!("Can't detach data channel: {}", e);
                                        }
                                    };
                                })
                            })
                        })
                        .await;
                })
            }))
            .await;
    }
}

impl<'a> StreamMuxer for Connection {
    type Substream = PollDataChannel;
    type Error = Error;

    fn poll_inbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        match ready!(self.incoming_data_channels_rx.poll_next(cx)) {
            Some(detached) => {
                trace!("Incoming substream {}", detached.stream_identifier());

                let mut ch = PollDataChannel::new(detached);
                if let Some(cap) = self.read_buf_cap {
                    ch.set_read_buf_capacity(cap);
                }

                Poll::Ready(Ok(ch))
            }
            None => Poll::Ready(Err(Error::InternalError(
                "incoming_data_channels_rx is closed (no messages left)".to_string(),
            ))),
        }
    }

    fn poll_address_change(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Multiaddr, Self::Error>> {
        return Poll::Pending;
    }

    fn poll_outbound(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>> {
        let peer_conn = self.peer_conn.clone();
        let fut = self.outbound_fut.get_or_insert(Box::pin(async move {
            let peer_conn = peer_conn.lock().await;

            // Create a datachannel with label 'data'
            let data_channel = peer_conn
                .create_data_channel("data", None)
                .map_err(Error::WebRTC)
                .await?;

            trace!("Opening outbound substream {}", data_channel.id());

            // No need to hold the lock during the DTLS handshake.
            drop(peer_conn);

            let (tx, rx) = oneshot::channel::<Arc<DetachedDataChannel>>();

            // Wait until the data channel is opened and detach it.
            register_data_channel_open_handler(data_channel, tx).await;

            // Wait until data channel is opened and ready to use
            match rx.await {
                Ok(detached) => Ok(detached),
                Err(e) => Err(Error::InternalError(e.to_string())),
            }
        }));

        match ready!(fut.as_mut().poll(cx)) {
            Ok(detached) => {
                let mut ch = PollDataChannel::new(detached);
                if let Some(cap) = self.read_buf_cap {
                    ch.set_read_buf_capacity(cap);
                }
                self.outbound_fut = None;
                Poll::Ready(Ok(ch))
            }
            Err(e) => {
                self.outbound_fut = None;
                Poll::Ready(Err(e))
            }
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        debug!("Closing connection");

        let peer_conn = self.peer_conn.clone();
        let fut = self.close_fut.get_or_insert(Box::pin(async move {
            let peer_conn = peer_conn.lock().await;
            peer_conn.close().await.map_err(Error::WebRTC)
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
    data_channel
        .on_open({
            let data_channel = data_channel.clone();
            Box::new(move || {
                debug!(
                    "Data channel '{}'-'{}' open",
                    data_channel.label(),
                    data_channel.id()
                );

                Box::pin(async move {
                    let data_channel = data_channel.clone();
                    match data_channel.detach().await {
                        Ok(detached) => {
                            if let Err(e) = data_channel_tx.send(detached.clone()) {
                                error!("Can't send data channel: {:?}", e);
                                if let Err(e) = detached.close().await {
                                    error!("Failed to close data channel: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Can't detach data channel: {}", e);
                        }
                    };
                })
            })
        })
        .await;
}
