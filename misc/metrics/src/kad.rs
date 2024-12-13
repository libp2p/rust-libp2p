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

pub(crate) struct Metrics {
    query_result_get_record_ok: Counter,
    query_result_get_record_error: Family<GetRecordResult, Counter>,

    query_result_get_closest_peers_ok: Histogram,
    query_result_get_closest_peers_error: Family<GetClosestPeersResult, Counter>,

    query_result_get_providers_ok: Histogram,
    query_result_get_providers_error: Family<GetProvidersResult, Counter>,

    query_result_num_requests: Family<QueryResult, Histogram>,
    query_result_num_success: Family<QueryResult, Histogram>,
    query_result_num_failure: Family<QueryResult, Histogram>,
    query_result_duration: Family<QueryResult, Histogram>,

    routing_updated: Family<RoutingUpdated, Counter>,

    inbound_requests: Family<InboundRequest, Counter>,
}

impl Metrics {
    pub(crate) fn new(registry: &mut Registry) -> Self {
        let sub_registry = registry.sub_registry_with_prefix("kad");

        let query_result_get_record_ok = Counter::default();
        sub_registry.register(
            "query_result_get_record_ok",
            "Number of records returned by a successful Kademlia get record query",
            query_result_get_record_ok.clone(),
        );

        let query_result_get_record_error = Family::default();
        sub_registry.register(
            "query_result_get_record_error",
            "Number of failed Kademlia get record queries",
            query_result_get_record_error.clone(),
        );

        let query_result_get_closest_peers_ok = Histogram::new(exponential_buckets(1.0, 2.0, 10));
        sub_registry.register(
            "query_result_get_closest_peers_ok",
            "Number of closest peers returned by a successful Kademlia get closest peers query",
            query_result_get_closest_peers_ok.clone(),
        );

        let query_result_get_closest_peers_error = Family::default();
        sub_registry.register(
            "query_result_get_closest_peers_error",
            "Number of failed Kademlia get closest peers queries",
            query_result_get_closest_peers_error.clone(),
        );

        let query_result_get_providers_ok = Histogram::new(exponential_buckets(1.0, 2.0, 10));
        sub_registry.register(
            "query_result_get_providers_ok",
            "Number of providers returned by a successful Kademlia get providers query",
            query_result_get_providers_ok.clone(),
        );

        let query_result_get_providers_error = Family::default();
        sub_registry.register(
            "query_result_get_providers_error",
            "Number of failed Kademlia get providers queries",
            query_result_get_providers_error.clone(),
        );

        let query_result_num_requests: Family<_, _> =
            Family::new_with_constructor(|| Histogram::new(exponential_buckets(1.0, 2.0, 10)));
        sub_registry.register(
            "query_result_num_requests",
            "Number of requests started for a Kademlia query",
            query_result_num_requests.clone(),
        );

        let query_result_num_success: Family<_, _> =
            Family::new_with_constructor(|| Histogram::new(exponential_buckets(1.0, 2.0, 10)));
        sub_registry.register(
            "query_result_num_success",
            "Number of successful requests of a Kademlia query",
            query_result_num_success.clone(),
        );

        let query_result_num_failure: Family<_, _> =
            Family::new_with_constructor(|| Histogram::new(exponential_buckets(1.0, 2.0, 10)));
        sub_registry.register(
            "query_result_num_failure",
            "Number of failed requests of a Kademlia query",
            query_result_num_failure.clone(),
        );

        let query_result_duration: Family<_, _> =
            Family::new_with_constructor(|| Histogram::new(exponential_buckets(0.1, 2.0, 10)));
        sub_registry.register_with_unit(
            "query_result_duration",
            "Duration of a Kademlia query",
            Unit::Seconds,
            query_result_duration.clone(),
        );

        let routing_updated = Family::default();
        sub_registry.register(
            "routing_updated",
            "Number of peers added, updated or evicted to, in or from a specific kbucket in the routing table",
            routing_updated.clone(),
        );

        let inbound_requests = Family::default();
        sub_registry.register(
            "inbound_requests",
            "Number of inbound requests",
            inbound_requests.clone(),
        );

        Self {
            query_result_get_record_ok,
            query_result_get_record_error,

            query_result_get_closest_peers_ok,
            query_result_get_closest_peers_error,

            query_result_get_providers_ok,
            query_result_get_providers_error,

            query_result_num_requests,
            query_result_num_success,
            query_result_num_failure,
            query_result_duration,

            routing_updated,

            inbound_requests,
        }
    }
}

impl super::Recorder<libp2p_kad::Event> for Metrics {
    fn record(&self, event: &libp2p_kad::Event) {
        match event {
            libp2p_kad::Event::OutboundQueryProgressed { result, stats, .. } => {
                self.query_result_num_requests
                    .get_or_create(&result.into())
                    .observe(stats.num_requests().into());
                self.query_result_num_success
                    .get_or_create(&result.into())
                    .observe(stats.num_successes().into());
                self.query_result_num_failure
                    .get_or_create(&result.into())
                    .observe(stats.num_failures().into());
                if let Some(duration) = stats.duration() {
                    self.query_result_duration
                        .get_or_create(&result.into())
                        .observe(duration.as_secs_f64());
                }

                match result {
                    libp2p_kad::QueryResult::GetRecord(result) => match result {
                        Ok(libp2p_kad::GetRecordOk::FoundRecord(_)) => {
                            self.query_result_get_record_ok.inc();
                        }
                        Ok(libp2p_kad::GetRecordOk::FinishedWithNoAdditionalRecord { .. }) => {}
                        Err(error) => {
                            self.query_result_get_record_error
                                .get_or_create(&error.into())
                                .inc();
                        }
                    },
                    libp2p_kad::QueryResult::GetClosestPeers(result) => match result {
                        Ok(ok) => self
                            .query_result_get_closest_peers_ok
                            .observe(ok.peers.len() as f64),
                        Err(error) => {
                            self.query_result_get_closest_peers_error
                                .get_or_create(&error.into())
                                .inc();
                        }
                    },
                    libp2p_kad::QueryResult::GetProviders(result) => match result {
                        Ok(libp2p_kad::GetProvidersOk::FoundProviders { providers, .. }) => {
                            self.query_result_get_providers_ok
                                .observe(providers.len() as f64);
                        }
                        Ok(libp2p_kad::GetProvidersOk::FinishedWithNoAdditionalRecord {
                            ..
                        }) => {}
                        Err(error) => {
                            self.query_result_get_providers_error
                                .get_or_create(&error.into())
                                .inc();
                        }
                    },
                    _ => {}
                }
            }
            libp2p_kad::Event::RoutingUpdated {
                is_new_peer,
                old_peer,
                bucket_range: (low, _high),
                ..
            } => {
                let bucket = low.ilog2().unwrap_or(0);
                if *is_new_peer {
                    self.routing_updated
                        .get_or_create(&RoutingUpdated {
                            action: RoutingAction::Added,
                            bucket,
                        })
                        .inc();
                } else {
                    self.routing_updated
                        .get_or_create(&RoutingUpdated {
                            action: RoutingAction::Updated,
                            bucket,
                        })
                        .inc();
                }

                if old_peer.is_some() {
                    self.routing_updated
                        .get_or_create(&RoutingUpdated {
                            action: RoutingAction::Evicted,
                            bucket,
                        })
                        .inc();
                }
            }

            libp2p_kad::Event::InboundRequest { request } => {
                self.inbound_requests.get_or_create(&request.into()).inc();
            }
            _ => {}
        }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct QueryResult {
    r#type: QueryType,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum QueryType {
    Bootstrap,
    GetClosestPeers,
    GetProviders,
    StartProviding,
    RepublishProvider,
    GetRecord,
    PutRecord,
    RepublishRecord,
}

impl From<&libp2p_kad::QueryResult> for QueryResult {
    fn from(result: &libp2p_kad::QueryResult) -> Self {
        match result {
            libp2p_kad::QueryResult::Bootstrap(_) => QueryResult {
                r#type: QueryType::Bootstrap,
            },
            libp2p_kad::QueryResult::GetClosestPeers(_) => QueryResult {
                r#type: QueryType::GetClosestPeers,
            },
            libp2p_kad::QueryResult::GetProviders(_) => QueryResult {
                r#type: QueryType::GetProviders,
            },
            libp2p_kad::QueryResult::StartProviding(_) => QueryResult {
                r#type: QueryType::StartProviding,
            },
            libp2p_kad::QueryResult::RepublishProvider(_) => QueryResult {
                r#type: QueryType::RepublishProvider,
            },
            libp2p_kad::QueryResult::GetRecord(_) => QueryResult {
                r#type: QueryType::GetRecord,
            },
            libp2p_kad::QueryResult::PutRecord(_) => QueryResult {
                r#type: QueryType::PutRecord,
            },
            libp2p_kad::QueryResult::RepublishRecord(_) => QueryResult {
                r#type: QueryType::RepublishRecord,
            },
        }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct GetRecordResult {
    error: GetRecordError,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum GetRecordError {
    NotFound,
    QuorumFailed,
    Timeout,
}

impl From<&libp2p_kad::GetRecordError> for GetRecordResult {
    fn from(error: &libp2p_kad::GetRecordError) -> Self {
        match error {
            libp2p_kad::GetRecordError::NotFound { .. } => GetRecordResult {
                error: GetRecordError::NotFound,
            },
            libp2p_kad::GetRecordError::QuorumFailed { .. } => GetRecordResult {
                error: GetRecordError::QuorumFailed,
            },
            libp2p_kad::GetRecordError::Timeout { .. } => GetRecordResult {
                error: GetRecordError::Timeout,
            },
        }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct GetClosestPeersResult {
    error: GetClosestPeersError,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum GetClosestPeersError {
    Timeout,
}

impl From<&libp2p_kad::GetClosestPeersError> for GetClosestPeersResult {
    fn from(error: &libp2p_kad::GetClosestPeersError) -> Self {
        match error {
            libp2p_kad::GetClosestPeersError::Timeout { .. } => GetClosestPeersResult {
                error: GetClosestPeersError::Timeout,
            },
        }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct GetProvidersResult {
    error: GetProvidersError,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum GetProvidersError {
    Timeout,
}

impl From<&libp2p_kad::GetProvidersError> for GetProvidersResult {
    fn from(error: &libp2p_kad::GetProvidersError) -> Self {
        match error {
            libp2p_kad::GetProvidersError::Timeout { .. } => GetProvidersResult {
                error: GetProvidersError::Timeout,
            },
        }
    }
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct RoutingUpdated {
    action: RoutingAction,
    bucket: u32,
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum RoutingAction {
    Added,
    Updated,
    Evicted,
}

#[derive(EncodeLabelSet, Hash, Clone, Eq, PartialEq, Debug)]
struct InboundRequest {
    request: Request,
}

impl From<&libp2p_kad::InboundRequest> for InboundRequest {
    fn from(request: &libp2p_kad::InboundRequest) -> Self {
        Self {
            request: match request {
                libp2p_kad::InboundRequest::FindNode { .. } => Request::FindNode,
                libp2p_kad::InboundRequest::GetProvider { .. } => Request::GetProvider,
                libp2p_kad::InboundRequest::AddProvider { .. } => Request::AddProvider,
                libp2p_kad::InboundRequest::GetRecord { .. } => Request::GetRecord,
                libp2p_kad::InboundRequest::PutRecord { .. } => Request::PutRecord,
            },
        }
    }
}

#[derive(EncodeLabelValue, Hash, Clone, Eq, PartialEq, Debug)]
enum Request {
    FindNode,
    GetProvider,
    AddProvider,
    GetRecord,
    PutRecord,
}
