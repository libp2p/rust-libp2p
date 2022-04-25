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

use prometheus_client::metrics::counter::Counter;
use prometheus_client::registry::Registry;

pub struct Metrics {
    messages: Counter,
}

impl Metrics {
    pub fn new(registry: &mut Registry) -> Self {
        let sub_registry = registry.sub_registry_with_prefix("gossipsub");

        let messages = Counter::default();
        sub_registry.register(
            "messages",
            "Number of messages received",
            Box::new(messages.clone()),
        );

        Self { messages }
    }
}

impl super::Recorder<libp2p_gossipsub::GossipsubEvent> for super::Metrics {
    fn record(&self, event: &libp2p_gossipsub::GossipsubEvent) {
        if let libp2p_gossipsub::GossipsubEvent::Message { .. } = event {
            self.gossipsub.messages.inc();
        }
    }
}
