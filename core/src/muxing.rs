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

pub use self::boxed::StreamMuxerBox;
pub use self::boxed::SubstreamBox;
pub use self::singleton::SingletonMuxer;

mod boxed;
mod singleton;

/// Provides multiplexing for a connection by allowing users to open substreams.
///
/// A substream created by a [`StreamMuxer`] is a type that implements [`AsyncRead`] and [`AsyncWrite`].
///
/// The process new incoming substreams and open new outbound ones, call the [`StreamMuxer::poll_event`]
/// function with the appropriate [`OpenFlags`].
pub trait StreamMuxer {
    /// Type of the object that represents the raw substream where data can be read and written.
    type Substream: AsyncRead + AsyncWrite;

    /// Error type of the muxer
    type Error: std::error::Error;

    /// Polls for a connection-wide event.
    ///
    /// Depending on the passed [`OpenFlags`], the muxer will either open a new outbound substream,
    /// check for new incoming substreams or both.
    ///
    /// It is permissible and common to use this method to perform background
    /// work, such as processing incoming packets and polling timers.
    ///
    /// An error can be generated if the connection has been closed.
    fn poll_event(
        &self,
        flags: OpenFlags,
        cx: &mut Context<'_>,
    ) -> Poll<Result<StreamMuxerEvent<Self::Substream>, Self::Error>>;

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
    fn poll_close(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>>;
}

bitflags::bitflags! {
    /// Tells the [`StreamMuxer`] which substreams it should open.
    ///
    /// How this is implemented may vary from muxer to muxer. Not all muxing protocols support
    /// this on the lowest level so some muxers may for example still accept incoming substreams but
    /// immediately drop them if the flags do not contain [`OpenFlags::INBOUND`].
    pub struct OpenFlags: u8 {
        const INBOUND  = 0b00000001;
        const OUTBOUND = 0b00000010;
    }
}

impl Default for OpenFlags {
    fn default() -> Self {
        OpenFlags::INBOUND
    }
}

/// Event about a connection, reported by an implementation of [`StreamMuxer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamMuxerEvent<T> {
    /// Remote has opened a new substream. Contains the substream in question.
    InboundSubstream(T),

    /// We opened a new substream. Contains the substream in question.
    OutboundSubstream(T),

    /// Address to the remote has changed. The previous one is now obsolete.
    ///
    /// > **Note**: This can for example happen when using the QUIC protocol, where the two nodes
    /// >           can change their IP address while retaining the same QUIC connection.
    AddressChange(Multiaddr),
}

impl<T> StreamMuxerEvent<T> {
    /// If `self` is a [`StreamMuxerEvent::InboundSubstream`], returns the content. Otherwise
    /// returns `None`.
    pub fn into_inbound_substream(self) -> Option<T> {
        if let StreamMuxerEvent::InboundSubstream(s) = self {
            Some(s)
        } else {
            None
        }
    }

    /// If `self` is a [`StreamMuxerEvent::OutboundSubstream`], returns the content. Otherwise
    /// returns `None`.
    pub fn into_outbound_substream(self) -> Option<T> {
        if let StreamMuxerEvent::OutboundSubstream(s) = self {
            Some(s)
        } else {
            None
        }
    }

    /// Map the stream within [`StreamMuxerEvent::InboundSubstream`] and
    /// [`StreamMuxerEvent::OutboundSubstream`] to a new type.
    pub fn map_stream<O>(self, map: impl FnOnce(T) -> O) -> StreamMuxerEvent<O> {
        match self {
            StreamMuxerEvent::InboundSubstream(stream) => {
                StreamMuxerEvent::InboundSubstream(map(stream))
            }
            StreamMuxerEvent::OutboundSubstream(stream) => {
                StreamMuxerEvent::OutboundSubstream(map(stream))
            }
            StreamMuxerEvent::AddressChange(addr) => StreamMuxerEvent::AddressChange(addr),
        }
    }
}
