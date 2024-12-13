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

//! Provides the `TransportExt` trait.

use std::sync::Arc;

use libp2p_identity::PeerId;

#[allow(deprecated)]
use crate::bandwidth::{BandwidthLogging, BandwidthSinks};
use crate::{
    core::{
        muxing::{StreamMuxer, StreamMuxerBox},
        transport::Boxed,
    },
    Transport,
};

/// Trait automatically implemented on all objects that implement `Transport`. Provides some
/// additional utilities.
pub trait TransportExt: Transport {
    /// Adds a layer on the `Transport` that logs all traffic that passes through the streams
    /// created by it.
    ///
    /// This method returns an `Arc<BandwidthSinks>` that can be used to retrieve the total number
    /// of bytes transferred through the streams.
    ///
    /// # Example
    ///
    /// ```
    /// use libp2p::{core::upgrade, identity, Transport, TransportExt};
    /// use libp2p_noise as noise;
    /// use libp2p_tcp as tcp;
    /// use libp2p_yamux as yamux;
    ///
    /// let id_keys = identity::Keypair::generate_ed25519();
    ///
    /// let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
    ///     .upgrade(upgrade::Version::V1)
    ///     .authenticate(
    ///         noise::Config::new(&id_keys).expect("Signing libp2p-noise static DH keypair failed."),
    ///     )
    ///     .multiplex(yamux::Config::default())
    ///     .boxed();
    ///
    /// let (transport, sinks) = transport.with_bandwidth_logging();
    /// ```
    #[allow(deprecated)]
    #[deprecated(
        note = "Use `libp2p::SwarmBuilder::with_bandwidth_metrics` or `libp2p_metrics::BandwidthTransport` instead."
    )]
    fn with_bandwidth_logging<S>(self) -> (Boxed<(PeerId, StreamMuxerBox)>, Arc<BandwidthSinks>)
    where
        Self: Sized + Send + Unpin + 'static,
        Self::Dial: Send + 'static,
        Self::ListenerUpgrade: Send + 'static,
        Self::Error: Send + Sync,
        Self::Output: Into<(PeerId, S)>,
        S: StreamMuxer + Send + 'static,
        S::Substream: Send + 'static,
        S::Error: Send + Sync + 'static,
    {
        let sinks = BandwidthSinks::new();
        let sinks_copy = sinks.clone();
        let transport = Transport::map(self, |output, _| {
            let (peer_id, stream_muxer_box) = output.into();
            (
                peer_id,
                StreamMuxerBox::new(BandwidthLogging::new(stream_muxer_box, sinks_copy)),
            )
        })
        .boxed();
        (transport, sinks)
    }
}

impl<TTransport> TransportExt for TTransport where TTransport: Transport {}
