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

//! Auxiliary crate recording protocol and Swarm events and exposing them as
//! metrics in the [OpenMetrics] format.
//!
//! [OpenMetrics]: https://github.com/OpenObservability/OpenMetrics/
//!
//! See `examples` directory for more.

#[cfg(feature = "dcutr")]
mod dcutr;
#[cfg(feature = "gossipsub")]
#[cfg(not(target_os = "unknown"))]
mod gossipsub;
#[cfg(feature = "identify")]
mod identify;
#[cfg(feature = "kad")]
mod kad;
#[cfg(feature = "ping")]
mod ping;
#[cfg(feature = "relay")]
mod relay;
mod swarm;

use prometheus_client::registry::Registry;

/// Set of Swarm and protocol metrics derived from emitted events.
pub struct Metrics {
    #[cfg(feature = "dcutr")]
    dcutr: dcutr::Metrics,
    #[cfg(feature = "gossipsub")]
    #[cfg(not(target_os = "unknown"))]
    gossipsub: gossipsub::Metrics,
    #[cfg(feature = "identify")]
    identify: identify::Metrics,
    #[cfg(feature = "kad")]
    kad: kad::Metrics,
    #[cfg(feature = "ping")]
    ping: ping::Metrics,
    #[cfg(feature = "relay")]
    relay: relay::Metrics,
    swarm: swarm::Metrics,
}

impl Metrics {
    /// Create a new set of Swarm and protocol [`Metrics`].
    ///
    /// ```
    /// use prometheus_client::registry::Registry;
    /// use libp2p_metrics::Metrics;
    /// let mut registry = Registry::default();
    /// let metrics = Metrics::new(&mut registry);
    /// ```
    pub fn new(registry: &mut Registry) -> Self {
        let sub_registry = registry.sub_registry_with_prefix("libp2p");
        Self {
            #[cfg(feature = "dcutr")]
            dcutr: dcutr::Metrics::new(sub_registry),
            #[cfg(feature = "gossipsub")]
            #[cfg(not(target_os = "unknown"))]
            gossipsub: gossipsub::Metrics::new(sub_registry),
            #[cfg(feature = "identify")]
            identify: identify::Metrics::new(sub_registry),
            #[cfg(feature = "kad")]
            kad: kad::Metrics::new(sub_registry),
            #[cfg(feature = "ping")]
            ping: ping::Metrics::new(sub_registry),
            #[cfg(feature = "relay")]
            relay: relay::Metrics::new(sub_registry),
            swarm: swarm::Metrics::new(sub_registry),
        }
    }
}

/// Recorder that can record Swarm and protocol events.
pub trait Recorder<Event> {
    /// Record the given event.
    fn record(&self, event: &Event);
}

#[cfg(feature = "dcutr")]
impl Recorder<libp2p_dcutr::behaviour::Event> for Metrics {
    fn record(&self, event: &libp2p_dcutr::behaviour::Event) {
        self.dcutr.record(event)
    }
}

#[cfg(feature = "gossipsub")]
#[cfg(not(target_os = "unknown"))]
impl Recorder<libp2p_gossipsub::GossipsubEvent> for Metrics {
    fn record(&self, event: &libp2p_gossipsub::GossipsubEvent) {
        self.gossipsub.record(event)
    }
}

#[cfg(feature = "identify")]
impl Recorder<libp2p_identify::IdentifyEvent> for Metrics {
    fn record(&self, event: &libp2p_identify::IdentifyEvent) {
        self.identify.record(event)
    }
}

#[cfg(feature = "kad")]
impl Recorder<libp2p_kad::KademliaEvent> for Metrics {
    fn record(&self, event: &libp2p_kad::KademliaEvent) {
        self.kad.record(event)
    }
}

#[cfg(feature = "ping")]
impl Recorder<libp2p_ping::PingEvent> for Metrics {
    fn record(&self, event: &libp2p_ping::PingEvent) {
        self.ping.record(event)
    }
}

#[cfg(feature = "relay")]
impl Recorder<libp2p_relay::v2::relay::Event> for Metrics {
    fn record(&self, event: &libp2p_relay::v2::relay::Event) {
        self.relay.record(event)
    }
}

impl<TBvEv, THandleErr> Recorder<libp2p_swarm::SwarmEvent<TBvEv, THandleErr>> for Metrics {
    fn record(&self, event: &libp2p_swarm::SwarmEvent<TBvEv, THandleErr>) {
        self.swarm.record(event);

        #[cfg(feature = "identify")]
        self.identify.record(event)
    }
}
