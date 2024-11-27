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

use prometheus_client::{
    encoding::{EncodeLabelSet, EncodeLabelValue},
    metrics::{counter::Counter, family::Family},
    registry::Registry,
};

pub(crate) struct Metrics {
    events: Family<EventLabels, Counter>,
}

impl Metrics {
    pub(crate) fn new(registry: &mut Registry) -> Self {
        let sub_registry = registry.sub_registry_with_prefix("dcutr");

        let events = Family::default();
        sub_registry.register(
            "events",
            "Events emitted by the relay NetworkBehaviour",
            events.clone(),
        );

        Self { events }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct EventLabels {
    event: EventType,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelValue)]
enum EventType {
    DirectConnectionUpgradeSucceeded,
    DirectConnectionUpgradeFailed,
}

impl From<&libp2p_dcutr::Event> for EventType {
    fn from(event: &libp2p_dcutr::Event) -> Self {
        match event {
            libp2p_dcutr::Event {
                remote_peer_id: _,
                result: Ok(_),
            } => EventType::DirectConnectionUpgradeSucceeded,
            libp2p_dcutr::Event {
                remote_peer_id: _,
                result: Err(_),
            } => EventType::DirectConnectionUpgradeFailed,
        }
    }
}

impl super::Recorder<libp2p_dcutr::Event> for Metrics {
    fn record(&self, event: &libp2p_dcutr::Event) {
        self.events
            .get_or_create(&EventLabels {
                event: event.into(),
            })
            .inc();
    }
}
