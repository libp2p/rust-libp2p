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
    metrics::{
        counter::Counter,
        family::Family,
        histogram::{exponential_buckets, Histogram},
    },
    registry::{Registry, Unit},
};

#[derive(Clone, Hash, PartialEq, Eq, EncodeLabelSet, Debug)]
struct FailureLabels {
    reason: Failure,
}

impl From<&libp2p_ping::Failure> for FailureLabels {
    fn from(failure: &libp2p_ping::Failure) -> Self {
        match failure {
            libp2p_ping::Failure::Timeout => FailureLabels {
                reason: Failure::Timeout,
            },
            libp2p_ping::Failure::Unsupported => FailureLabels {
                reason: Failure::Unsupported,
            },
            libp2p_ping::Failure::Other { .. } => FailureLabels {
                reason: Failure::Other,
            },
        }
    }
}

#[derive(Clone, Hash, PartialEq, Eq, EncodeLabelValue, Debug)]
enum Failure {
    Timeout,
    Unsupported,
    Other,
}

pub(crate) struct Metrics {
    rtt: Histogram,
    failure: Family<FailureLabels, Counter>,
}

impl Metrics {
    pub(crate) fn new(registry: &mut Registry) -> Self {
        let sub_registry = registry.sub_registry_with_prefix("ping");

        let rtt = Histogram::new(exponential_buckets(0.001, 2.0, 12));
        sub_registry.register_with_unit(
            "rtt",
            "Round-trip time sending a 'ping' and receiving a 'pong'",
            Unit::Seconds,
            rtt.clone(),
        );

        let failure = Family::default();
        sub_registry.register(
            "failure",
            "Failure while sending a 'ping' or receiving a 'pong'",
            failure.clone(),
        );

        Self { rtt, failure }
    }
}

impl super::Recorder<libp2p_ping::Event> for Metrics {
    fn record(&self, event: &libp2p_ping::Event) {
        match &event.result {
            Ok(rtt) => {
                self.rtt.observe(rtt.as_secs_f64());
            }
            Err(failure) => {
                self.failure.get_or_create(&failure.into()).inc();
            }
        }
    }
}
