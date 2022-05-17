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

use futures::prelude::*;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc_data::data_channel::DataChannel;

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use poll_data_channel::PollDataChannel;

/// A WebRTC connection over a single data channel. See lib documentation for
/// the reasoning as to why a single data channel is being used.
pub struct Connection<'a> {
    /// `RTCPeerConnection` to the remote peer.
    pub inner: RTCPeerConnection,
    /// A data channel.
    pub data_channel: PollDataChannel<'a>,
}

impl Connection<'_> {
    pub fn new(peer_conn: RTCPeerConnection, data_channel: Arc<DataChannel>) -> Self {
        Self {
            inner: peer_conn,
            data_channel: PollDataChannel::new(data_channel),
        }
    }
}

impl AsyncRead for Connection<'_> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.data_channel).poll_read(cx, buf)
    }
}

impl AsyncWrite for Connection<'_> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.data_channel).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.data_channel).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.data_channel).poll_close(cx)
    }
}
