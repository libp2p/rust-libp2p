use async_std::task;
use futures::channel::{mpsc, oneshot};
use futures::future::{poll_fn, FutureExt};
use futures::sink::SinkExt;
use futures::stream::{Stream, StreamExt};
use libp2p::autonat::{AutoNat, AutoNatEvent, DialResponse};
use libp2p::core::{
    identity, muxing::StreamMuxerBox, transport::Transport, upgrade, Multiaddr, PeerId,
};
use libp2p::noise::{Keypair, NoiseConfig, X25519Spec};
use libp2p::swarm::Swarm;
use libp2p::tcp::TcpConfig;
use netsim_embed::{Ipv4Range, NatConfig, NetworkBuilder};
use std::{io, pin::Pin, task::Poll};

fn mk_swarm() -> (PeerId, Swarm<AutoNat>) {
    let id_keys = identity::Keypair::generate_ed25519();
    let peer_id = id_keys.public().into_peer_id();
    let noise_keys = Keypair::<X25519Spec>::new()
        .into_authentic(&id_keys)
        .expect("invalid noise keys");
    let transport = TcpConfig::new()
        .nodelay(true)
        .upgrade(upgrade::Version::V1)
        .authenticate(NoiseConfig::xx(noise_keys).into_authenticated())
        .multiplex(libp2p::yamux::Config::default())
        .map(|(peer, muxer), _| (peer, StreamMuxerBox::new(muxer)))
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
        .boxed();
    let swarm = Swarm::new(transport, AutoNat::default(), peer_id.clone());
    (peer_id, swarm)
}

enum Command {
    DialRequest(PeerId, Multiaddr),
}

enum Event {
    Ready(PeerId, Multiaddr),
    Result(AutoNatEvent),
}

fn autonat_netsim_setup(natconfig: NatConfig) -> oneshot::Receiver<AutoNatEvent> {
    let (tx, rx) = oneshot::channel();
    netsim_embed::run(async move {
        let mut global = NetworkBuilder::new(Ipv4Range::global());
        global.spawn_machine(
            |mut cmd: mpsc::Receiver<Command>, mut event: mpsc::Sender<Event>| async move {
                let (peer_id, mut swarm) = mk_swarm();
                let addr = "/ip4/0.0.0.0/tcp/0".parse().expect("Invalid addr");
                Swarm::listen_on(&mut swarm, addr).expect("listen_on failed");
                while let Some(_) = swarm.next().now_or_never() {}
                let listener = Swarm::listeners(&swarm).next().expect("no listeners");
                event
                    .send(Event::Ready(peer_id, listener.clone()))
                    .await
                    .expect("failed to send ready event");

                poll_fn(move |cx| loop {
                    if let Poll::Ready(None) = Pin::new(&mut cmd).poll_next(cx) {
                        return Poll::Ready(());
                    }
                    if let Poll::Pending = Pin::new(&mut swarm).poll_next(cx) {
                        return Poll::Pending;
                    }
                })
                .await
            },
        );

        let mut local = NetworkBuilder::new(Ipv4Range::random_local_subnet());
        local.spawn_machine(
            |mut cmd: mpsc::Receiver<Command>, mut event: mpsc::Sender<Event>| async move {
                let (_peer_id, mut swarm) = mk_swarm();
                let addr = "/ip4/0.0.0.0/tcp/0".parse().expect("Invalid addr");
                Swarm::listen_on(&mut swarm, addr).expect("listen_on failed");
                while let Some(_) = swarm.next().now_or_never() {}

                if let Some(Command::DialRequest(peer_id, addr)) = cmd.next().await {
                    swarm.add_address(&peer_id, addr);
                    swarm.dial_request(&peer_id);
                } else {
                    unreachable!();
                }
                let result = swarm.next().await;
                event
                    .send(Event::Result(result))
                    .await
                    .expect("sending result failed");
            },
        );

        global.spawn_network(Some(natconfig), local);

        let mut net = global.spawn();
        let (peer_id, addr) = if let Event::Ready(peer_id, addr) =
            net.machine(0).recv().await.expect("recv ready failed")
        {
            (peer_id, addr)
        } else {
            unreachable!()
        };

        let m = net.subnet(0).machine(0);
        m.send(Command::DialRequest(peer_id, addr)).await;
        if let Event::Result(event) = m.recv().await.expect("recv result failed") {
            tx.send(event).expect("sending result failed 2");
        }
    });
    rx
}

#[test]
#[ignore]
fn autonat_with_traversable_nat() {
    let natconfig = NatConfig::default();
    let rx = autonat_netsim_setup(natconfig);
    task::block_on(async move {
        let event = rx.await.expect("recv result failed 2");
        if let AutoNatEvent::DialResponse(DialResponse::Ok(_)) = event {
        } else {
            panic!();
        }
    });
}

#[test]
#[ignore]
fn autonat_with_untraversable_nat() {
    let mut natconfig = NatConfig::default();
    natconfig.symmetric = true;
    let rx = autonat_netsim_setup(natconfig);
    task::block_on(async move {
        let event = rx.await.expect("recv result failed 2");
        if let AutoNatEvent::DialResponse(DialResponse::Err(_, _)) = event {
        } else {
            panic!();
        }
    });
}
