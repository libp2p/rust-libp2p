use std::{convert::Infallible, sync::atomic::AtomicUsize};

use async_std::stream::StreamExt;
use criterion::{criterion_group, criterion_main, Criterion};
use libp2p_core::{
    transport::MemoryTransport, InboundUpgrade, Multiaddr, OutboundUpgrade, Transport, UpgradeInfo,
};
use libp2p_identity::PeerId;
use libp2p_swarm::{ConnectionHandler, NetworkBehaviour, StreamProtocol};
use web_time::Duration;

macro_rules! gen_behaviour {
    ($($name:ident {$($field:ident),*};)*) => {$(
        #[derive(libp2p_swarm::NetworkBehaviour, Default)]
        #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
        struct $name {
            $($field: SpinningBehaviour,)*
        }

        impl BigBehaviour for $name {
            fn behaviours(&mut self) -> &mut [SpinningBehaviour] {
                unsafe {
                    std::slice::from_raw_parts_mut(
                        self as *mut Self as *mut SpinningBehaviour,
                        std::mem::size_of::<Self>() / std::mem::size_of::<SpinningBehaviour>(),
                    )
                }
            }
        }
    )*};
}

macro_rules! benchmarks {
    ($(
        $group:ident::[$(
            $beh:ident::bench()
                .name($name:ident)
                .poll_count($count:expr)
                .protocols_per_behaviour($protocols:expr),
        )+];
    )*) => {

        $(
            $(
                fn $name(c: &mut Criterion) {
                    <$beh>::run_bench(c, $protocols, $count, true);
                }
            )+

            criterion_group!($group, $($name),*);
        )*

        criterion_main!($($group),*);
    };
}

// fans go brrr
gen_behaviour! {
    SpinningBehaviour5 { a, b, c, d, e };
    SpinningBehaviour10 { a, b, c, d, e, f, g, h, i, j };
    SpinningBehaviour20 { a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q, r, s, t, u };
}

benchmarks! {
    singles::[
        SpinningBehaviour::bench().name(b).poll_count(1000).protocols_per_behaviour(10),
        SpinningBehaviour::bench().name(c).poll_count(1000).protocols_per_behaviour(100),
        SpinningBehaviour::bench().name(d).poll_count(1000).protocols_per_behaviour(1000),
    ];
    big_5::[
        SpinningBehaviour5::bench().name(e).poll_count(1000).protocols_per_behaviour(2),
        SpinningBehaviour5::bench().name(f).poll_count(1000).protocols_per_behaviour(20),
        SpinningBehaviour5::bench().name(g).poll_count(1000).protocols_per_behaviour(200),
    ];
    top_10::[
        SpinningBehaviour10::bench().name(h).poll_count(1000).protocols_per_behaviour(1),
        SpinningBehaviour10::bench().name(i).poll_count(1000).protocols_per_behaviour(10),
        SpinningBehaviour10::bench().name(j).poll_count(1000).protocols_per_behaviour(100),
    ];
    lucky_20::[
        SpinningBehaviour20::bench().name(k).poll_count(500).protocols_per_behaviour(1),
        SpinningBehaviour20::bench().name(l).poll_count(500).protocols_per_behaviour(10),
        SpinningBehaviour20::bench().name(m).poll_count(500).protocols_per_behaviour(100),
    ];
}
// fn main() {}

trait BigBehaviour: Sized {
    fn behaviours(&mut self) -> &mut [SpinningBehaviour];

    fn for_each_beh(&mut self, f: impl FnMut(&mut SpinningBehaviour)) {
        self.behaviours().iter_mut().for_each(f);
    }

    fn any_beh(&mut self, f: impl FnMut(&mut SpinningBehaviour) -> bool) -> bool {
        self.behaviours().iter_mut().any(f)
    }

    fn run_bench(
        c: &mut Criterion,
        protocols_per_behaviour: usize,
        spam_count: usize,
        static_protocols: bool,
    ) where
        Self: Default + NetworkBehaviour,
    {
        let name = format!(
            "{}::bench().poll_count({}).protocols_per_behaviour({})",
            std::any::type_name::<Self>(),
            spam_count,
            protocols_per_behaviour
        );

        let init = || {
            let mut swarm_a = new_swarm(Self::default());
            let mut swarm_b = new_swarm(Self::default());

            let behaviour_count = swarm_a.behaviours().len();
            let protocol_count = behaviour_count * protocols_per_behaviour;
            let protocols = (0..protocol_count)
                .map(|i| {
                    if static_protocols {
                        StreamProtocol::new(format!("/protocol/{i}").leak())
                    } else {
                        StreamProtocol::try_from_owned(format!("/protocol/{i}")).unwrap()
                    }
                })
                .collect::<Vec<_>>()
                .leak();

            let mut protocol_chunks = protocols.chunks(protocols_per_behaviour);
            swarm_a.for_each_beh(|b| b.protocols = protocol_chunks.next().unwrap());
            let mut protocol_chunks = protocols.chunks(protocols_per_behaviour);
            swarm_b.for_each_beh(|b| b.protocols = protocol_chunks.next().unwrap());

            swarm_a.for_each_beh(|b| b.iter_count = spam_count);
            swarm_b.for_each_beh(|b| b.iter_count = 0);

            swarm_a.for_each_beh(|b| b.other_peer = Some(*swarm_b.local_peer_id()));
            swarm_b.for_each_beh(|b| b.other_peer = Some(*swarm_a.local_peer_id()));

            static OFFSET: AtomicUsize = AtomicUsize::new(8000);
            let offset = OFFSET.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

            swarm_a
                .listen_on(format!("/memory/{offset}").parse().unwrap())
                .unwrap();
            swarm_b
                .dial(format!("/memory/{offset}").parse::<Multiaddr>().unwrap())
                .unwrap();

            (swarm_a, swarm_b)
        };

        c.bench_function(&name, |b| {
            b.to_async(tokio::runtime::Builder::new_multi_thread().build().unwrap())
                .iter_batched(
                    init,
                    |(mut swarm_a, mut swarm_b)| async move {
                        while swarm_a.any_beh(|b| !b.finished) || swarm_b.any_beh(|b| !b.finished) {
                            futures::future::select(swarm_b.next(), swarm_a.next()).await;
                        }
                    },
                    criterion::BatchSize::LargeInput,
                );
        });
    }
}

impl<T: BigBehaviour + NetworkBehaviour> BigBehaviour for libp2p_swarm::Swarm<T> {
    fn behaviours(&mut self) -> &mut [SpinningBehaviour] {
        self.behaviour_mut().behaviours()
    }
}

fn new_swarm<T: NetworkBehaviour>(beh: T) -> libp2p_swarm::Swarm<T> {
    let keypair = libp2p_identity::Keypair::generate_ed25519();
    libp2p_swarm::Swarm::new(
        MemoryTransport::default()
            .upgrade(multistream_select::Version::V1)
            .authenticate(libp2p_plaintext::Config::new(&keypair))
            .multiplex(libp2p_yamux::Config::default())
            .boxed(),
        beh,
        keypair.public().to_peer_id(),
        libp2p_swarm::Config::without_executor().with_idle_connection_timeout(Duration::MAX),
    )
}

/// Whole purpose of the behaviour is to rapidly call `poll` on the handler
/// configured amount of times and then emit event when finished.
#[derive(Default)]
struct SpinningBehaviour {
    iter_count: usize,
    protocols: &'static [StreamProtocol],
    finished: bool,
    emitted: bool,
    other_peer: Option<PeerId>,
}

#[derive(Debug)]
struct FinishedSpinning;

impl NetworkBehaviour for SpinningBehaviour {
    type ConnectionHandler = SpinningHandler;
    type ToSwarm = FinishedSpinning;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        _peer: libp2p_identity::PeerId,
        _local_addr: &libp2p_core::Multiaddr,
        _remote_addr: &libp2p_core::Multiaddr,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(SpinningHandler {
            iter_count: 0,
            protocols: self.protocols,
        })
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p_swarm::ConnectionId,
        _peer: libp2p_identity::PeerId,
        _addr: &libp2p_core::Multiaddr,
        _role_override: libp2p_core::Endpoint,
        _port_use: libp2p_core::transport::PortUse,
    ) -> Result<libp2p_swarm::THandler<Self>, libp2p_swarm::ConnectionDenied> {
        Ok(SpinningHandler {
            iter_count: self.iter_count,
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
        self.finished = true;
    }

    fn poll(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_swarm::ToSwarm<Self::ToSwarm, libp2p_swarm::THandlerInEvent<Self>>>
    {
        if self.finished && !self.emitted {
            self.emitted = true;
            std::task::Poll::Ready(libp2p_swarm::ToSwarm::GenerateEvent(FinishedSpinning))
        } else {
            std::task::Poll::Pending
        }
    }
}

impl BigBehaviour for SpinningBehaviour {
    fn behaviours(&mut self) -> &mut [SpinningBehaviour] {
        std::slice::from_mut(self)
    }
}

struct SpinningHandler {
    iter_count: usize,
    protocols: &'static [StreamProtocol],
}

impl ConnectionHandler for SpinningHandler {
    type FromBehaviour = Infallible;

    type ToBehaviour = FinishedSpinning;

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
        if self.iter_count == usize::MAX {
            return std::task::Poll::Pending;
        }

        if self.iter_count != 0 {
            self.iter_count -= 1;
            cx.waker().wake_by_ref();
            return std::task::Poll::Pending;
        }

        self.iter_count = usize::MAX;
        std::task::Poll::Ready(libp2p_swarm::ConnectionHandlerEvent::NotifyBehaviour(
            FinishedSpinning,
        ))
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        match event {}
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
