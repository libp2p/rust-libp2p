/*
 * Copyright 2020 Fluence Labs Limited
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use prometheus::{IntCounterVec, IntGauge, Opts, Registry};

use libp2p_core::PeerId;
use libp2p_swarm::NetworkBehaviourAction;

use crate::handler::{KademliaHandlerEvent, KademliaHandlerIn};
use crate::{KademliaEvent, QueryId};

pub enum Kind {
    Request,
    Response,
    Error,
}

struct InnerMetrics {
    // Requests sent via NotifyHandler (KademliaHandlerIn)
    sent_requests: IntCounterVec,
    // Responses sent via NotifyHandler (KademliaHandlerIn)
    sent_responses: IntCounterVec,
    // Requests received via inject_event
    received_requests: IntCounterVec,
    // Responses received via inject_event
    received_responses: IntCounterVec,
    errors: IntCounterVec,
    records_stored: IntGauge,
    connected_nodes: IntGauge,
    routing_table_size: IntGauge,
    // Kademlia events (except NotifyHandler)
    kademlia_events: IntCounterVec,
    // TODO: nodes received in queries?
}

enum Inner {
    Disabled,
    Enabled(InnerMetrics),
}

pub struct Metrics {
    inner: Inner,
}

impl Metrics {
    pub fn disabled() -> Self {
        Self {
            inner: Inner::Disabled,
        }
    }

    pub fn enabled(registry: &Registry, peer_id: &PeerId) -> Self {
        let peer_id = bs58::encode(peer_id).into_string();
        let opts = |name: &str| -> Opts {
            let mut opts = Opts::new(name, name)
                .namespace("libp2p")
                .subsystem("kad")
                .const_label("peer_id", peer_id.as_str());
            opts.name = name.into();
            opts.help = name.into(); // TODO: better help?
            opts
        };

        // Creates and registers counter in registry
        let counter = |name: &str, label_names: &[&str]| -> IntCounterVec {
            let counter = IntCounterVec::new(opts(name), label_names)
                .expect(format!("create {}", name).as_str());

            registry
                .register(Box::new(counter.clone()))
                .expect(format!("register {}", name).as_str());
            counter
        };

        let gauge = |name: &str| -> IntGauge {
            let gauge = IntGauge::with_opts(opts(name)).expect(format!("create {}", name).as_str());
            registry
                .register(Box::new(gauge.clone()))
                .expect(format!("register {}", name).as_str());
            gauge
        };

        // let requests = &["find_node", "get_providers", "add_provider", "get_record", "put_record"];
        // let responses = &["find_node", "get_providers", "get_record", "put_record"];
        // let errors = &["todo"]; // TODO: fill error types

        let name = &["name"];

        let sent_requests = counter("sent_requests", name);
        let received_requests = counter("received_requests", name);
        let sent_responses = counter("sent_responses", name);
        let received_responses = counter("received_responses", name);
        let kademlia_events = counter("kademlia_event", name);
        let errors = counter("errors", name);
        let records_stored = gauge("records_stored");
        let connected_nodes = gauge("connected_nodes");
        let routing_table_size = gauge("routing_table_size");

        Self {
            inner: Inner::Enabled(InnerMetrics {
                sent_requests,
                received_responses,
                received_requests,
                sent_responses,
                errors,
                records_stored,
                connected_nodes,
                kademlia_events,
                routing_table_size,
            }),
        }
    }

    fn with_metrics<F>(&self, f: F)
    where
        F: FnOnce(&InnerMetrics),
    {
        if let Inner::Enabled(metrics) = &self.inner {
            f(metrics)
        }
    }

    fn inc_by_name(name: &str, counter: &IntCounterVec) {
        match counter.get_metric_with_label_values(&[name]) {
            Ok(c) => c.inc(),
            Err(e) => log::warn!("failed to increment counter {}: {:?}", name, e),
        }
    }

    pub fn node_connected(&self) {
        self.with_metrics(|m| m.connected_nodes.inc());
    }

    pub fn received(&self, event: &KademliaHandlerEvent<QueryId>) {
        use Kind::*;

        let (name, kind) = match event {
            // requests
            KademliaHandlerEvent::FindNodeReq { .. } => ("find_node_req", Request),
            KademliaHandlerEvent::GetProvidersReq { .. } => ("get_providers_req", Request),
            KademliaHandlerEvent::AddProvider { .. } => ("add_provider", Request),
            KademliaHandlerEvent::GetRecord { .. } => ("get_record", Request),
            KademliaHandlerEvent::PutRecord { .. } => ("put_record", Request),

            // responses
            KademliaHandlerEvent::FindNodeRes { .. } => ("find_node_res", Response),
            KademliaHandlerEvent::GetProvidersRes { .. } => ("get_providers_res", Response),
            KademliaHandlerEvent::GetRecordRes { .. } => ("get_record_res", Response),
            KademliaHandlerEvent::PutRecordRes { .. } => ("put_record_res", Response),

            // error
            KademliaHandlerEvent::QueryError { .. } => ("query_error_from_handler", Error),
        };

        self.with_metrics(|m| {
            let counter = match &kind {
                Request => &m.sent_requests,
                Response => &m.sent_responses,
                Error => &m.errors,
            };

            Self::inc_by_name(name, counter);
        });
    }

    pub fn sent(&self, event: &KademliaHandlerIn<QueryId>) {
        use Kind::*;

        let (name, kind) = match event {
            // requests
            KademliaHandlerIn::FindNodeReq { .. } => ("find_node", Request),
            KademliaHandlerIn::GetProvidersReq { .. } => ("get_providers", Request),
            KademliaHandlerIn::AddProvider { .. } => ("add_provider", Request),
            KademliaHandlerIn::GetRecord { .. } => ("get_record", Request),
            KademliaHandlerIn::PutRecord { .. } => ("put_record", Request),

            // responses
            KademliaHandlerIn::GetProvidersRes { .. } => ("get_providers", Request),
            KademliaHandlerIn::GetRecordRes { .. } => ("get_record", Request),
            KademliaHandlerIn::PutRecordRes { .. } => ("put_record", Request),
            KademliaHandlerIn::FindNodeRes { .. } => ("find_node", Request),

            // error
            KademliaHandlerIn::Reset(_) => ("sent_reset", Request),
        };

        self.with_metrics(|m| {
            let counter = match &kind {
                Request => &m.received_requests,
                Response => &m.received_responses,
                Error => &m.errors,
            };

            Self::inc_by_name(name, counter);
        });
    }

    pub fn generated_event_name(event: &KademliaEvent) -> &str {
        match event {
            KademliaEvent::QueryResult { .. } => "query_result",
            KademliaEvent::Discovered { .. } => "discovered",
            KademliaEvent::RoutingUpdated { .. } => "routing_updated",
            KademliaEvent::UnroutablePeer { .. } => "unroutable_peer",
        }
    }

    pub fn polled_event(
        &self,
        event: &NetworkBehaviourAction<KademliaHandlerIn<QueryId>, KademliaEvent>,
    ) {
        let name = match event {
            NetworkBehaviourAction::DialAddress { .. } => "dial_address",
            NetworkBehaviourAction::DialPeer { .. } => "dial_peer",
            NetworkBehaviourAction::ReportObservedAddr { .. } => "report_observed_addr",
            NetworkBehaviourAction::GenerateEvent(e) => Self::generated_event_name(e),
            NetworkBehaviourAction::NotifyHandler { event, .. } => {
                self.sent(event);
                return;
            }
        };

        self.with_metrics(|m| Self::inc_by_name(name, &m.kademlia_events));
    }

    pub fn store_put(&self) {
        self.with_metrics(|m| m.records_stored.inc())
    }

    pub fn record_removed(&self) {
        self.with_metrics(|m| m.records_stored.dec())
    }

    pub fn report_routing_table_size(&self, size: usize) {
        self.with_metrics(|m| m.routing_table_size.set(size as i64))
    }
}
