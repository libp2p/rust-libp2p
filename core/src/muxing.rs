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
    fn poll_inbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>>;

    /// Poll for a new, outbound substream.
    fn poll_outbound(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::Substream, Self::Error>>;

    /// Poll for an address change of the underlying connection.
    ///
    /// Not all implementations may support this feature.
    fn poll_address_change(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Multiaddr, Self::Error>>;

    /// Closes this `StreamMuxer`.
    ///
    /// After this has returned `Poll::Ready(Ok(()))`, the muxer has become useless. All
    /// subsequent reads must return either `EOF` or an error. All subsequent writes, shutdowns,
    /// or polls must generate an error or be ignored.
    ///
    /// > **Note**: You are encouraged to call this method and wait for it to return `Ready`, so
    /// >           that the remote is properly informed of the shutdown. However, apart from
    /// >           properly informing the remote, there is no difference between this and
    /// >           immediately dropping the muxer.
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
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

    /// Convenience function for calling [`StreamMuxer::poll_address_change`] for [`StreamMuxer`]s that are `Unpin`.
    fn poll_address_change_unpin(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Multiaddr, Self::Error>>
    where
        Self: Unpin,
    {
        Pin::new(self).poll_address_change(cx)
    }

    /// Convenience function for calling [`StreamMuxer::poll_close`] for [`StreamMuxer`]s that are `Unpin`.
    fn poll_close_unpin(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>
    where
        Self: Unpin,
    {
        Pin::new(self).poll_close(cx)
    }

    /// Returns a future that resolves to the next inbound `Substream` opened by the remote.
    fn next_inbound(&mut self) -> NextInbound<'_, Self> {
        NextInbound(self)
    }

    /// Returns a future that opens a new outbound `Substream` with the remote.
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
