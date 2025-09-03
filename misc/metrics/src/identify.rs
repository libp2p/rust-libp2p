// Copyright 2021 Protocol Labs.
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

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use libp2p_identity::PeerId;
use libp2p_swarm::StreamProtocol;
use prometheus_client::{
    collector::Collector,
    encoding::{DescriptorEncoder, EncodeMetric},
    metrics::{counter::Counter, gauge::ConstGauge, MetricType},
    registry::Registry,
};

use crate::protocol_stack;

const ALLOWED_PROTOCOLS: &[StreamProtocol] = &[
    #[cfg(feature = "dcutr")]
    libp2p_dcutr::PROTOCOL_NAME,
    // #[cfg(feature = "gossipsub")]
    // TODO: Add Gossipsub protocol name
    libp2p_identify::PROTOCOL_NAME,
    libp2p_identify::PUSH_PROTOCOL_NAME,
    #[cfg(feature = "kad")]
    libp2p_kad::PROTOCOL_NAME,
    #[cfg(feature = "ping")]
    libp2p_ping::PROTOCOL_NAME,
    #[cfg(feature = "relay")]
    libp2p_relay::STOP_PROTOCOL_NAME,
    #[cfg(feature = "relay")]
    libp2p_relay::HOP_PROTOCOL_NAME,
];

pub(crate) struct Metrics {
    peers: Peers,
    error: Counter,
    pushed: Counter,
    received: Counter,
    sent: Counter,
}

impl Metrics {
    pub(crate) fn new(registry: &mut Registry) -> Self {
        let sub_registry = registry.sub_registry_with_prefix("identify");

        let peers = Peers::default();
        sub_registry.register_collector(Box::new(peers.clone()));

        let error = Counter::default();
        sub_registry.register(
            "errors",
            "Number of errors while attempting to identify the remote",
            error.clone(),
        );

        let pushed = Counter::default();
        sub_registry.register(
            "pushed",
            "Number of times identification information of the local node has \
             been actively pushed to a peer.",
            pushed.clone(),
        );

        let received = Counter::default();
        sub_registry.register(
            "received",
            "Number of times identification information has been received from \
             a peer",
            received.clone(),
        );

        let sent = Counter::default();
        sub_registry.register(
            "sent",
            "Number of times identification information of the local node has \
             been sent to a peer in response to an identification request",
            sent.clone(),
        );

        Self {
            peers,
            error,
            pushed,
            received,
            sent,
        }
    }
}

impl super::Recorder<libp2p_identify::Event> for Metrics {
    fn record(&self, event: &libp2p_identify::Event) {
        match event {
            libp2p_identify::Event::Error { .. } => {
                self.error.inc();
            }
            libp2p_identify::Event::Pushed { .. } => {
                self.pushed.inc();
            }
            libp2p_identify::Event::Received { peer_id, info, .. } => {
                self.received.inc();
                self.peers.record(*peer_id, info.clone());
            }
            libp2p_identify::Event::Sent { .. } => {
                self.sent.inc();
            }
        }
    }
}

impl<TBvEv> super::Recorder<libp2p_swarm::SwarmEvent<TBvEv>> for Metrics {
    fn record(&self, event: &libp2p_swarm::SwarmEvent<TBvEv>) {
        if let libp2p_swarm::SwarmEvent::ConnectionClosed {
            peer_id,
            num_established,
            ..
        } = event
        {
            if *num_established == 0 {
                self.peers.remove(*peer_id);
            }
        }
    }
}

#[derive(Default, Debug, Clone)]
struct Peers(Arc<Mutex<HashMap<PeerId, libp2p_identify::Info>>>);

impl Peers {
    fn record(&self, peer_id: PeerId, info: libp2p_identify::Info) {
        self.0.lock().unwrap().insert(peer_id, info);
    }

    fn remove(&self, peer_id: PeerId) {
        self.0.lock().unwrap().remove(&peer_id);
    }
}

impl Collector for Peers {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), std::fmt::Error> {
        let mut count_by_protocols: HashMap<String, i64> = Default::default();
        let mut count_by_listen_addresses: HashMap<String, i64> = Default::default();
        let mut count_by_observed_addresses: HashMap<String, i64> = Default::default();

        for (_, peer_info) in self.0.lock().unwrap().iter() {
            {
                let mut protocols: Vec<_> = peer_info
                    .protocols
                    .iter()
                    .map(|p| {
                        if ALLOWED_PROTOCOLS.contains(p) {
                            p.to_string()
                        } else {
                            "unrecognized".to_string()
                        }
                    })
                    .collect();
                protocols.sort();
                protocols.dedup();

                for protocol in protocols.into_iter() {
                    let count = count_by_protocols.entry(protocol).or_default();
                    *count += 1;
                }
            }

            {
                let mut addrs: Vec<_> = peer_info
                    .listen_addrs
                    .iter()
                    .map(protocol_stack::as_string)
                    .collect();
                addrs.sort();
                addrs.dedup();

                for addr in addrs {
                    let count = count_by_listen_addresses.entry(addr).or_default();
                    *count += 1;
                }
            }

            {
                let count = count_by_observed_addresses
                    .entry(protocol_stack::as_string(&peer_info.observed_addr))
                    .or_default();
                *count += 1;
            }
        }

        {
            let mut family_encoder = encoder.encode_descriptor(
                "remote_protocols",
                "Number of connected nodes supporting a specific protocol, with \"unrecognized\" for each peer supporting one or more unrecognized protocols",
                None,
                MetricType::Gauge,
            )?;
            for (protocol, count) in count_by_protocols.into_iter() {
                let labels = [("protocol", protocol)];
                let metric_encoder = family_encoder.encode_family(&labels)?;
                let metric = ConstGauge::new(count);
                metric.encode(metric_encoder)?;
            }
        }

        {
            let mut family_encoder = encoder.encode_descriptor(
                "remote_listen_addresses",
                "Number of connected nodes advertising a specific listen address",
                None,
                MetricType::Gauge,
            )?;
            for (protocol, count) in count_by_listen_addresses.into_iter() {
                let labels = [("listen_address", protocol)];
                let metric_encoder = family_encoder.encode_family(&labels)?;
                ConstGauge::new(count).encode(metric_encoder)?;
            }
        }

        {
            let mut family_encoder = encoder.encode_descriptor(
                "local_observed_addresses",
                "Number of connected nodes observing the local node at a specific address",
                None,
                MetricType::Gauge,
            )?;
            for (protocol, count) in count_by_observed_addresses.into_iter() {
                let labels = [("observed_address", protocol)];
                let metric_encoder = family_encoder.encode_family(&labels)?;
                ConstGauge::new(count).encode(metric_encoder)?;
            }
        }

        Ok(())
    }
}
