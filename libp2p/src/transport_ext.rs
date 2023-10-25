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

use crate::core::{
    muxing::{StreamMuxer, StreamMuxerBox},
    transport::Boxed,
};
use crate::{
    bandwidth::{BandwidthLogging, BandwidthSinks},
    Transport,
};
use libp2p_identity::PeerId;
use multiaddr::Multiaddr;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

/// Trait automatically implemented on all objects that implement `Transport`. Provides some
/// additional utilities.
pub trait TransportExt: Transport {
    /// Adds a layer on the `Transport` that logs all trafic that passes through the streams
    /// created by it.
    ///
    /// This method returns an `Arc<BandwidthSinks>` that can be used to retrieve the total number
    /// of bytes transferred through the streams.
    ///
    /// # Example
    ///
    /// ```
    /// use libp2p_yamux as yamux;
    /// use libp2p_noise as noise;
    /// use libp2p_tcp as tcp;
    /// use libp2p::{
    ///     core::upgrade,
    ///     identity,
    ///     TransportExt,
    ///     Transport,
    /// };
    ///
    /// let id_keys = identity::Keypair::generate_ed25519();
    ///
    /// let transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true))
    ///     .upgrade(upgrade::Version::V1)
    ///     .authenticate(
    ///         noise::Config::new(&id_keys)
    ///             .expect("Signing libp2p-noise static DH keypair failed."),
    ///     )
    ///     .multiplex(yamux::Config::default())
    ///     .boxed();
    ///
    /// let (transport, sinks) = transport.with_bandwidth_logging();
    /// ```
    fn with_bandwidth_logging<S>(
        self,
    ) -> (
        Boxed<(PeerId, StreamMuxerBox)>,
        Arc<RwLock<HashMap<String, Arc<BandwidthSinks>>>>,
    )
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
        let sinks: Arc<RwLock<HashMap<_, Arc<BandwidthSinks>>>> = Arc::new(RwLock::new(HashMap::new()));
        let sinks_copy = sinks.clone();
        let transport = Transport::map(self, move |output, connected_point| {
            fn as_string(ma: &Multiaddr) -> String {
                let len = ma
                    .protocol_stack()
                    .fold(0, |acc, proto| acc + proto.len() + 1);
                let mut protocols = String::with_capacity(len);
                for proto_tag in ma.protocol_stack() {
                    protocols.push('/');
                    protocols.push_str(proto_tag);
                }
                protocols
            }

            let sink = sinks_copy
                .write()
                .expect("todo")
                .entry(as_string(connected_point.get_remote_address()))
                .or_default()
                .clone();

            let (peer_id, stream_muxer_box) = output.into();
            (
                peer_id,
                StreamMuxerBox::new(BandwidthLogging::new(stream_muxer_box, sink)),
            )
        })
        .boxed();
        (transport, sinks)
    }
}

impl<TTransport> TransportExt for TTransport where TTransport: Transport {}
