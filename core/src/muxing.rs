// Copyright 2018 Parity Technologies (UK) Ltd.
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

//! Muxing is the process of splitting a connection into multiple substreams.
//!
//! The main item of this module is the `StreamMuxer` trait. An implementation of `StreamMuxer`
//! has ownership of a connection, lets you open and close substreams.
//!
//! > **Note**: You normally don't need to use the methods of the `StreamMuxer` directly, as this
//! >           is managed by the library's internals.
//!
//! Each substream of a connection is an isolated stream of data. All the substreams are muxed
//! together so that the data read from or written to each substream doesn't influence the other
//! substreams.
//!
//! In the context of libp2p, each substream can use a different protocol. Contrary to opening a
//! connection, opening a substream is almost free in terms of resources. This means that you
//! shouldn't hesitate to rapidly open and close substreams, and to design protocols that don't
//! require maintaining long-lived channels of communication.
//!
//! > **Example**: The Kademlia protocol opens a new substream for each request it wants to
//! >              perform. Multiple requests can be performed simultaneously by opening multiple
//! >              substreams, without having to worry about associating responses with the
//! >              right request.
//!
//! # Implementing a muxing protocol
//!
//! In order to implement a muxing protocol, create an object that implements the `UpgradeInfo`,
//! `InboundUpgrade` and `OutboundUpgrade` traits. See the `upgrade` module for more information.
//! The `Output` associated type of the `InboundUpgrade` and `OutboundUpgrade` traits should be
//! identical, and should be an object that implements the `StreamMuxer` trait.
//!
//! The upgrade process will take ownership of the connection, which makes it possible for the
//! implementation of `StreamMuxer` to control everything that happens on the wire.

use futures::{task::Context, task::Poll, AsyncRead, AsyncWrite};
use multiaddr::Multiaddr;
use std::future::Future;
use std::pin::Pin;

pub use self::boxed::StreamMuxerBox;
pub use self::boxed::SubstreamBox;
pub use self::singleton::SingletonMuxer;

mod boxed;
mod singleton;

/// Provides multiplexing for a connection by allowing users to open substreams.
///
/// A substream created by a [`StreamMuxer`] is a type that implements [`AsyncRead`] and [`AsyncWrite`].
/// The [`StreamMuxer`] itself is modelled closely after [`AsyncWrite`]. It features `poll`-style
/// functions that allow the implementation to make progress on various tasks.
pub trait StreamMuxer {
    /// Type of the object that represents the raw substream where data can be read and written.
    type Substream: AsyncRead + AsyncWrite;

    /// Error type of the muxer
    type Error: std::error::Error;

    /// Poll for new inbound substreams.
    ///
    /// This function should be called whenever callers are ready to accept more inbound streams. In
    /// other words, callers may exercise back-pressure on incoming streams by not calling this
    /// function if a certain limit is hit.
    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>>;

    /// Poll for a new, outbound substream.
    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>>;

    /// Poll to close this [`StreamMuxer`].
    ///
    /// After this has returned `Poll::Ready(Ok(()))`, the muxer has become useless and may be safely
    /// dropped.
    ///
    /// > **Note**: You are encouraged to call this method and wait for it to return `Ready`, so
    /// >           that the remote is properly informed of the shutdown. However, apart from
    /// >           properly informing the remote, there is no difference between this and
    /// >           immediately dropping the muxer.
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;

    /// Poll to allow the underlying connection to make progress.
    ///
    /// In contrast to all other `poll`-functions on [`StreamMuxer`], this function MUST be called
    /// unconditionally. Because it will be called regardless, this function can be used by
    /// implementations to return events about the underlying connection that the caller MUST deal
    /// with.
    fn poll(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent, Self::Error>>;
}

/// An event produced by a [`StreamMuxer`].
pub enum StreamMuxerEvent {
    /// The address of the remote has changed.
    AddressChange(Multiaddr),
}

/// Extension trait for [`StreamMuxer`].
pub trait StreamMuxerExt: StreamMuxer + Sized {
    /// Convenience function for calling [`StreamMuxer::poll_inbound`] for [`StreamMuxer`]s that are `Unpin`.
    fn poll_inbound_unpin(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>>
    where
        Self: Unpin,
    {
        Pin::new(self).poll_inbound(cx)
    }

    /// Convenience function for calling [`StreamMuxer::poll_outbound`] for [`StreamMuxer`]s that are `Unpin`.
    fn poll_outbound_unpin(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>>
    where
        Self: Unpin,
    {
        Pin::new(self).poll_outbound(cx)
    }

    /// Convenience function for calling [`StreamMuxer::poll`] for [`StreamMuxer`]s that are `Unpin`.
    fn poll_unpin(&mut self, cx: &mut Context<'_>) -> Poll<Result<StreamMuxerEvent, Self::Error>>
    where
        Self: Unpin,
    {
        Pin::new(self).poll(cx)
    }

    /// Convenience function for calling [`StreamMuxer::poll_close`] for [`StreamMuxer`]s that are `Unpin`.
    fn poll_close_unpin(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>
    where
        Self: Unpin,
    {
        Pin::new(self).poll_close(cx)
    }

    /// Returns a future that resolves to the next inbound `Substream` opened by the remote.
    #[deprecated(
        since = "0.37.0",
        note = "This future violates the `StreamMuxer` contract because it doesn't call `StreamMuxer::poll`."
    )]
    fn next_inbound(&mut self) -> NextInbound<'_, Self> {
        NextInbound(self)
    }

    /// Returns a future that opens a new outbound `Substream` with the remote.
    #[deprecated(
        since = "0.37.0",
        note = "This future violates the `StreamMuxer` contract because it doesn't call `StreamMuxer::poll`."
    )]
    fn next_outbound(&mut self) -> NextOutbound<'_, Self> {
        NextOutbound(self)
    }

    /// Returns a future for closing this [`StreamMuxer`].
    fn close(self) -> Close<Self> {
        Close(self)
    }
}

impl<S> StreamMuxerExt for S where S: StreamMuxer {}

pub struct NextInbound<'a, S>(&'a mut S);

pub struct NextOutbound<'a, S>(&'a mut S);

pub struct Close<S>(S);

impl<'a, S> Future for NextInbound<'a, S>
where
    S: StreamMuxer + Unpin,
{
    type Output = Result<S::Substream, S::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.poll_inbound_unpin(cx)
    }
}

impl<'a, S> Future for NextOutbound<'a, S>
where
    S: StreamMuxer + Unpin,
{
    type Output = Result<S::Substream, S::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.poll_outbound_unpin(cx)
    }
}

impl<S> Future for Close<S>
where
    S: StreamMuxer + Unpin,
{
    type Output = Result<(), S::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.0.poll_close_unpin(cx)
    }
}
