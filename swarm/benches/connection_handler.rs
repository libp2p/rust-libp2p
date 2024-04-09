use async_std::stream::StreamExt;
use criterion::{criterion_group, criterion_main, Criterion};
use futures::FutureExt;
use instant::Duration;
use libp2p_core::{
    transport::MemoryTransport, InboundUpgrade, Multiaddr, OutboundUpgrade, Transport, UpgradeInfo,
};
use libp2p_swarm::{ConnectionHandler, NetworkBehaviour, StreamProtocol};
use std::convert::Infallible;

criterion_main!(one_behavior_many_protocols);

macro_rules! benchmark_one_behaviour_many_protocols {
    ($(
        $name:ident,
    )*) => {
        $(
            #[tokio::main(flavor = "multi_thread", worker_threads = 1)]
            async fn $name(c: &mut Criterion) {
                let protocol_count = parse_counts(stringify!($name));
                bench_run_one_behaviour_many_protocols(c, protocol_count, 100000);
            }
        )*

        criterion_group!(
            one_behavior_many_protocols,
            $(
                $name,
            )*
        );
    };
}

benchmark_one_behaviour_many_protocols! {
    one_behavior_many_protocols_100,
    one_behavior_many_protocols_1000,
    one_behavior_many_protocols_10000,
}

fn parse_counts(name: &str) -> usize {
    name.split('_').next_back().unwrap().parse().unwrap()
}

fn new_swarm(protocol_count: usize, spam_count: usize) -> libp2p_swarm::Swarm<PollerBehaviour> {
    // we leak to simulate static protocols witch is the common case
    let protocols = (0..protocol_count)
        .map(|i| StreamProtocol::new(format!("/protocol/{i}").leak()))
        .collect::<Vec<_>>()
        .leak();

    let swarm_a_keypair = libp2p_identity::Keypair::generate_ed25519();
    libp2p_swarm::Swarm::new(
        MemoryTransport::new()
            .upgrade(multistream_select::Version::V1)
            .authenticate(libp2p_plaintext::Config::new(&swarm_a_keypair))
            .multiplex(libp2p_yamux::Config::default())
            .boxed(),
        PollerBehaviour {
            spam_count,
            protocols,
        },
        swarm_a_keypair.public().to_peer_id(),
        libp2p_swarm::Config::with_tokio_executor().with_idle_connection_timeout(Duration::MAX),
    )
}

fn bench_run_one_behaviour_many_protocols(
    c: &mut Criterion,
    protocol_count: usize,
    spam_count: usize,
) {
    let mut sa = new_swarm(protocol_count, spam_count);
    let mut sb = new_swarm(protocol_count, spam_count);

    static mut OFFSET: usize = 0;
    let offset = unsafe {
        OFFSET += 1;
        OFFSET
    };

    sa.listen_on(format!("/memory/{offset}").parse().unwrap())
        .unwrap();
    sb.dial(format!("/memory/{offset}").parse::<Multiaddr>().unwrap())
        .unwrap();

    c.bench_function(&format!("one_behavior_many_protocols_{protocol_count}_{spam_count}"), |b| {
            b.iter(|| {
                futures::executor::block_on(async {
                    let [mut af, mut bf] = [false; 2];
                    while !af || !bf {
                        futures::select! {
                            event = sb.next().fuse() => {
                                bf |= matches!(event, Some(libp2p_swarm::SwarmEvent::Behaviour(FinishedSpamming)));
                            }
                            event = sa.next().fuse() => {
                                af |= matches!(event, Some(libp2p_swarm::SwarmEvent::Behaviour(FinishedSpamming)));
                            }
                        }
                    }
                });
            });
        });
}

struct PollerBehaviour {
    spam_count: usize,
    protocols: &'static [StreamProtocol],
}

#[derive(Debug)]
struct FinishedSpamming;

impl NetworkBehaviour for PollerBehaviour {
    type ConnectionHandler = PollerHandler;
    type ToSwarm = FinishedSpamming;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        _peer: libp2p_identity::PeerId,
        _local_addr: &libp2p_core::Multiaddr,
        _remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(PollerHandler {
            spam_count: 0,
            protocols: self.protocols,
        })
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        _peer: libp2p_identity::PeerId,
        _addr: &libp2p_core::Multiaddr,
        _role_override: libp2p_core::Endpoint,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(PollerHandler {
            spam_count: self.spam_count,
            protocols: self.protocols,
        })
    }

    fn on_swarm_event(&mut self, _: libp2p_swarm::FromSwarm) {}

    fn on_connection_handler_event(
        &mut self,
        _peer_id: libp2p_identity::PeerId,
        _connection_id: libp2p_swarm::ConnectionId,
        _event: libp2p_swarm::THandlerOutEvent<Self>,
    ) {
        self.spam_count = 0;
    }

    fn poll(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_swarm::ToSwarm<Self::ToSwarm, libp2p_swarm::THandlerInEvent<Self>>>
    {
        if self.spam_count == 0 {
            std::task::Poll::Ready(libp2p_swarm::ToSwarm::GenerateEvent(FinishedSpamming))
        } else {
            std::task::Poll::Pending
        }
    }
}

#[derive(Default)]
struct PollerHandler {
    spam_count: usize,
    protocols: &'static [StreamProtocol],
}

impl ConnectionHandler for PollerHandler {
    type FromBehaviour = Infallible;

    type ToBehaviour = FinishedSpamming;

    type InboundProtocol = Upgrade;

    type OutboundProtocol = Upgrade;

    type InboundOpenInfo = ();

    type OutboundOpenInfo = ();

    fn listen_protocol(
        &self,
    ) -> libp2p_swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        libp2p_swarm::SubstreamProtocol::new(Upgrade(self.protocols), ())
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<
        libp2p_swarm::ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
        >,
    > {
        if self.spam_count != 0 {
            self.spam_count -= 1;
            cx.waker().wake_by_ref();
            return std::task::Poll::Pending;
        }

        self.spam_count = usize::MAX;
        std::task::Poll::Ready(libp2p_swarm::ConnectionHandlerEvent::NotifyBehaviour(
            FinishedSpamming,
        ))
    }

    fn on_behaviour_event(&mut self, _event: Self::FromBehaviour) {
        match _event {}
    }

    fn on_connection_event(
        &mut self,
        _event: libp2p_swarm::handler::ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
    }
}

pub struct Upgrade(&'static [StreamProtocol]);

impl UpgradeInfo for Upgrade {
    type Info = &'static StreamProtocol;
    type InfoIter = std::slice::Iter<'static, StreamProtocol>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.0.iter()
    }
}

impl OutboundUpgrade<libp2p_swarm::Stream> for Upgrade {
    type Output = libp2p_swarm::Stream;
    type Error = Infallible;
    type Future = futures::future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(self, s: libp2p_swarm::Stream, _: Self::Info) -> Self::Future {
        futures::future::ready(Ok(s))
    }
}

impl InboundUpgrade<libp2p_swarm::Stream> for Upgrade {
    type Output = libp2p_swarm::Stream;
    type Error = Infallible;
    type Future = futures::future::Ready<Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(self, s: libp2p_swarm::Stream, _: Self::Info) -> Self::Future {
        futures::future::ready(Ok(s))
    }
}
