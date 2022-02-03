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

use prometheus_client::encoding::text::Encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::family::Family;
use prometheus_client::registry::Registry;

pub struct Metrics {
    events: Family<EventLabels, Counter>,
}

impl Metrics {
    pub fn new(registry: &mut Registry) -> Self {
        let sub_registry = registry.sub_registry_with_prefix("relay");

        let events = Family::default();
        sub_registry.register(
            "events",
            "Events emitted by the relay NetworkBehaviour",
            Box::new(events.clone()),
        );

        Self { events }
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Encode)]
struct EventLabels {
    event: EventType,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Encode)]
enum EventType {
    ReservationReqAccepted,
    ReservationReqAcceptFailed,
    ReservationReqDenied,
    ReservationReqDenyFailed,
    ReservationTimedOut,
    CircuitReqReceiveFailed,
    CircuitReqDenied,
    CircuitReqDenyFailed,
    CircuitReqOutboundConnectFailed,
    CircuitReqAccepted,
    CircuitReqAcceptFailed,
    CircuitClosed,
}

impl From<&libp2p_relay::v2::relay::Event> for EventType {
    fn from(event: &libp2p_relay::v2::relay::Event) -> Self {
        match event {
            libp2p_relay::v2::relay::Event::ReservationReqAccepted { .. } => {
                EventType::ReservationReqAccepted
            }
            libp2p_relay::v2::relay::Event::ReservationReqAcceptFailed { .. } => {
                EventType::ReservationReqAcceptFailed
            }
            libp2p_relay::v2::relay::Event::ReservationReqDenied { .. } => {
                EventType::ReservationReqDenied
            }
            libp2p_relay::v2::relay::Event::ReservationReqDenyFailed { .. } => {
                EventType::ReservationReqDenyFailed
            }
            libp2p_relay::v2::relay::Event::ReservationTimedOut { .. } => {
                EventType::ReservationTimedOut
            }
            libp2p_relay::v2::relay::Event::CircuitReqReceiveFailed { .. } => {
                EventType::CircuitReqReceiveFailed
            }
            libp2p_relay::v2::relay::Event::CircuitReqDenied { .. } => EventType::CircuitReqDenied,
            libp2p_relay::v2::relay::Event::CircuitReqOutboundConnectFailed { .. } => {
                EventType::CircuitReqOutboundConnectFailed
            }
            libp2p_relay::v2::relay::Event::CircuitReqDenyFailed { .. } => {
                EventType::CircuitReqDenyFailed
            }
            libp2p_relay::v2::relay::Event::CircuitReqAccepted { .. } => {
                EventType::CircuitReqAccepted
            }
            libp2p_relay::v2::relay::Event::CircuitReqAcceptFailed { .. } => {
                EventType::CircuitReqAcceptFailed
            }
            libp2p_relay::v2::relay::Event::CircuitClosed { .. } => EventType::CircuitClosed,
        }
    }
}

impl super::Recorder<libp2p_relay::v2::relay::Event> for super::Metrics {
    fn record(&self, event: &libp2p_relay::v2::relay::Event) {
        self.relay
            .events
            .get_or_create(&EventLabels {
                event: event.into(),
            })
            .inc();
    }
}
