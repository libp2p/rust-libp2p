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

use crate::protocol_stack;
use libp2p_identity::PeerId;
use prometheus_client::encoding::{EncodeLabelSet, EncodeMetric, MetricEncoder};
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::metrics::histogram::{exponential_buckets, Histogram};
use prometheus_client::metrics::MetricType;
use prometheus_client::registry::Registry;
use std::collections::HashMap;
use std::iter;
use std::sync::{Arc, Mutex};

pub(crate) struct Metrics {
    protocols: Protocols,
    error: Counter,
    pushed: Counter,
    received: Counter,
    received_info_listen_addrs: Histogram,
    received_info_protocols: Histogram,
    sent: Counter,
    listen_addresses: Family<AddressLabels, Counter>,
}

impl Metrics {
    pub(crate) fn new(registry: &mut Registry) -> Self {
        let sub_registry = registry.sub_registry_with_prefix("identify");

        let protocols = Protocols::default();
        sub_registry.register(
            "protocols",
            "Number of connected nodes supporting a specific protocol, with \
             \"unrecognized\" for each peer supporting one or more unrecognized \
             protocols",
            protocols.clone(),
        );

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

        let received_info_listen_addrs =
            Histogram::new(iter::once(0.0).chain(exponential_buckets(1.0, 2.0, 9)));
        sub_registry.register(
            "received_info_listen_addrs",
            "Number of listen addresses for remote peer received in \
             identification information",
            received_info_listen_addrs.clone(),
        );

        let received_info_protocols =
            Histogram::new(iter::once(0.0).chain(exponential_buckets(1.0, 2.0, 9)));
        sub_registry.register(
            "received_info_protocols",
            "Number of protocols supported by the remote peer received in \
             identification information",
            received_info_protocols.clone(),
        );

        let sent = Counter::default();
        sub_registry.register(
            "sent",
            "Number of times identification information of the local node has \
             been sent to a peer in response to an identification request",
            sent.clone(),
        );

        let listen_addresses = Family::default();
        sub_registry.register(
            "listen_addresses",
            "Number of listen addresses for remote peer per protocol stack",
            listen_addresses.clone(),
        );

        Self {
            protocols,
            error,
            pushed,
            received,
            received_info_listen_addrs,
            received_info_protocols,
            sent,
            listen_addresses,
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
                {
                    let mut protocols: Vec<String> = info
                        .protocols
                        .iter()
                        .filter(|p| {
                            let allowed_protocols: &[&[u8]] = &[
                                #[cfg(feature = "dcutr")]
                                libp2p_dcutr::PROTOCOL_NAME,
                                // #[cfg(feature = "gossipsub")]
                                // #[cfg(not(target_os = "unknown"))]
                                // TODO: Add Gossipsub protocol name
                                libp2p_identify::PROTOCOL_NAME,
                                libp2p_identify::PUSH_PROTOCOL_NAME,
                                #[cfg(feature = "kad")]
                                libp2p_kad::protocol::DEFAULT_PROTO_NAME,
                                #[cfg(feature = "ping")]
                                libp2p_ping::PROTOCOL_NAME,
                                #[cfg(feature = "relay")]
                                libp2p_relay::STOP_PROTOCOL_NAME,
                                #[cfg(feature = "relay")]
                                libp2p_relay::HOP_PROTOCOL_NAME,
                            ];

                            allowed_protocols.contains(&p.as_bytes())
                        })
                        .cloned()
                        .collect();

                    // Signal via an additional label value that one or more
                    // protocols of the remote peer have not been recognized.
                    if protocols.len() < info.protocols.len() {
                        protocols.push("unrecognized".to_string());
                    }

                    protocols.sort_unstable();
                    protocols.dedup();

                    self.protocols.add(*peer_id, protocols);
                }

                self.received.inc();
                self.received_info_protocols
                    .observe(info.protocols.len() as f64);
                self.received_info_listen_addrs
                    .observe(info.listen_addrs.len() as f64);
                for listen_addr in &info.listen_addrs {
                    self.listen_addresses
                        .get_or_create(&AddressLabels {
                            protocols: protocol_stack::as_string(listen_addr),
                        })
                        .inc();
                }
            }
            libp2p_identify::Event::Sent { .. } => {
                self.sent.inc();
            }
        }
    }
}

impl<TBvEv, THandleErr> super::Recorder<libp2p_swarm::SwarmEvent<TBvEv, THandleErr>> for Metrics {
    fn record(&self, event: &libp2p_swarm::SwarmEvent<TBvEv, THandleErr>) {
        if let libp2p_swarm::SwarmEvent::ConnectionClosed {
            peer_id,
            num_established,
            ..
        } = event
        {
            if *num_established == 0 {
                self.protocols.remove(*peer_id)
            }
        }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct AddressLabels {
    protocols: String,
}

#[derive(Default, Clone, Debug)]
struct Protocols {
    peers: Arc<Mutex<HashMap<PeerId, Vec<String>>>>,
}

impl Protocols {
    fn add(&self, peer: PeerId, protocols: Vec<String>) {
        self.peers
            .lock()
            .expect("Lock not to be poisoned")
            .insert(peer, protocols);
    }

    fn remove(&self, peer: PeerId) {
        self.peers
            .lock()
            .expect("Lock not to be poisoned")
            .remove(&peer);
    }
}

impl EncodeMetric for Protocols {
    fn encode(&self, mut encoder: MetricEncoder) -> Result<(), std::fmt::Error> {
        let count_by_protocol = self
            .peers
            .lock()
            .expect("Lock not to be poisoned")
            .iter()
            .fold(
                HashMap::<String, i64>::default(),
                |mut acc, (_, protocols)| {
                    for protocol in protocols {
                        let count = acc.entry(protocol.to_string()).or_default();
                        *count += 1;
                    }
                    acc
                },
            );

        for (protocol, count) in count_by_protocol {
            encoder
                .encode_family(&[("protocol", protocol)])?
                .encode_gauge(&count)?;
        }

        Ok(())
    }

    fn metric_type(&self) -> MetricType {
        MetricType::Gauge
    }
}
