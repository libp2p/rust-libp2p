// Copyright 2018 Parity Technologies (UK) Ltd.
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

extern crate bigint;
extern crate bytes;
extern crate futures;
extern crate libp2p_floodsub;
extern crate libp2p_identify;
extern crate libp2p_kad;
extern crate libp2p_mplex;
extern crate libp2p_peerstore;
extern crate libp2p_core;
#[cfg(not(target_os = "emscripten"))]
extern crate libp2p_tcp_transport;
extern crate libp2p_websocket;
#[macro_use]
extern crate log;
extern crate rand;
extern crate simple_logger;
#[cfg(target_os = "emscripten")]
#[macro_use]
extern crate stdweb;
#[cfg(not(target_os = "emscripten"))]
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_stdin;
extern crate tokio_timer;

use bigint::U512;
use bytes::Bytes;
use futures::{Future, Stream};
use libp2p_floodsub::{FloodSubFuture, FloodSubUpgrade};
use libp2p_identify::{IdentifyInfo, IdentifyOutput, IdentifyProtocolConfig, IdentifyTransport};
use libp2p_kad::{KademliaProcessingFuture, KademliaUpgrade};
use libp2p_peerstore::{PeerAccess, PeerId, Peerstore};
use libp2p_core::{AddrComponent, ConnectionUpgrade, Endpoint, Multiaddr, Transport};
use std::fmt::Debug;
use std::io::Error as IoError;
use std::ops::Deref;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio_io::{AsyncRead, AsyncWrite};

fn main() {
    use futures::Stream;
    use std::env;

    simple_logger::init_with_level(log::Level::Info).expect("failed to initialize logging");

    let is_bootstrap = env::args().skip(1).next() == Some("bootstrap".to_owned());

    let bootstrap_peer_id =
        PeerId::from_public_key(include_bytes!("../bootstrap-node-public-key.der"));

    let peer_store = Arc::new(libp2p_peerstore::memory_peerstore::MemoryPeerstore::empty());
    if !is_bootstrap {
        peer_store.peer_or_create(&bootstrap_peer_id).add_addr(
            "/ip4/127.0.0.1/tcp/10101/ws".parse().unwrap(),
            Duration::from_secs(3600 * 24 * 365),
        );
    }

    let my_peer_id = if is_bootstrap {
        info!("Configured as a bootstrap node");
        bootstrap_peer_id
    } else {
        let key = (0..2048).map(|_| rand::random::<u8>()).collect::<Vec<_>>();
        PeerId::from_public_key(&key)
    };

    let to_listen: Option<Multiaddr> = if cfg!(not(target_os = "emscripten")) {
        Some(if is_bootstrap {
            "/ip4/0.0.0.0/tcp/10101/ws".parse().unwrap()
        } else {
            let addr = env::args()
                .skip(1)
                .next()
                .unwrap_or("/ip4/0.0.0.0/tcp/0/ws".to_owned());
            addr.parse()
                .expect("failed to parse multiaddress to listen to")
        })
    } else {
        None
    };

    let context = PlatformSpecific::default();
    let listened_addrs = Arc::new(RwLock::new(vec![]));

    let transport = context
        .build_transport()
        .with_upgrade(libp2p_core::upgrade::PlainTextConfig)
        .with_upgrade(libp2p_mplex::BufferedMultiplexConfig::<[_; 256]>::new())
        .into_connection_reuse();

    let transport = {
        let listened_addrs = listened_addrs.clone();
        let to_listen = to_listen.clone();
        IdentifyTransport::new(transport.clone(), peer_store.clone())
            .map(move |out, _, _| {
                if let (Some(ref observed), Some(ref to_listen)) = (out.observed_addr, to_listen) {
                    if let Some(viewed_from_outside) = transport.nat_traversal(to_listen, observed) {
                        listened_addrs.write().unwrap().push(viewed_from_outside);
                    }
                }
                out.socket
            })
    };

    info!("Local peer id is: {:?}", my_peer_id);

    let (floodsub_upgrade, floodsub_rx) = FloodSubUpgrade::new(my_peer_id.clone());

    // Let's put this `transport` into a Kademlia *swarm*. The swarm will handle all the incoming
    // and outgoing connections for us.
    let kad_config = libp2p_kad::KademliaConfig {
        parallelism: 3,
        record_store: (),
        peer_store: peer_store,
        local_peer_id: my_peer_id.clone(),
        timeout: Duration::from_secs(2),
    };

    let kad_ctl_proto = libp2p_kad::KademliaControllerPrototype::new(kad_config);
    let kad_upgrade = libp2p_kad::KademliaUpgrade::from_prototype(&kad_ctl_proto);

    let upgrade = ConnectionUpgrader {
        kad: kad_upgrade.clone(),
        identify: libp2p_identify::IdentifyProtocolConfig,
        floodsub: floodsub_upgrade.clone(),
    };

    let listened_addrs_clone = listened_addrs.clone();

    let my_peer_id_clone = my_peer_id.clone();
    let (swarm_controller, swarm_future) = libp2p_core::swarm(
        transport.clone().with_upgrade(upgrade),
        move |upgrade, client_addr| match upgrade {
            FinalUpgrade::Kad(kad) => Box::new(kad) as Box<_>,
            FinalUpgrade::FloodSub(future) => Box::new(future) as Box<_>,
            FinalUpgrade::Identify(IdentifyOutput::Sender { sender, .. }) => sender.send(
                IdentifyInfo {
                    public_key: my_peer_id_clone.clone().into_bytes(),
                    protocol_version: "chat/1.0.0".to_owned(),
                    agent_version: "rust-libp2p/1.0.0".to_owned(),
                    listen_addrs: listened_addrs_clone.read().unwrap().to_vec(),
                    protocols: vec![
                        "/ipfs/kad/1.0.0".to_owned(),
                        "/ipfs/id/1.0.0".to_owned(),
                        "/floodsub/1.0.0".to_owned(),
                    ],
                },
                &client_addr,
            ),
            FinalUpgrade::Identify(IdentifyOutput::RemoteInfo { .. }) => {
                unreachable!("We are never dialing with the identify protocol")
            }
        },
    );

    // Listening on the address passed as program argument, or a default one.
    if let Some(to_listen) = to_listen {
        let actual_addr = swarm_controller
            .listen_on(to_listen)
            .expect("failed to listen to multiaddress");
        info!("Now listening on {}", actual_addr);
        listened_addrs.write().unwrap().push(actual_addr);
    }

    let (kad_controller, kad_init) = kad_ctl_proto.start(swarm_controller.clone(), transport.clone().with_upgrade(kad_upgrade));

    let topic = libp2p_floodsub::TopicBuilder::new("chat").build();

    let floodsub_ctl = libp2p_floodsub::FloodSubController::new(&floodsub_upgrade);
    floodsub_ctl.subscribe(&topic);

    let floodsub_rx = floodsub_rx.for_each(|msg| {
        if let Ok(msg) = String::from_utf8(msg.data) {
            info!("< {}", msg);
        }
        Ok(())
    });

    let kad_poll = {
        let my_peer_id2 = my_peer_id.clone();
        tokio_timer::wheel()
            .build()
            .interval_at(Instant::now(), Duration::from_secs(30))
            .map_err(|_| -> IoError { unreachable!() })
            .and_then(move |()| kad_controller.find_node(my_peer_id.clone()))
            .for_each(move |out| {
                let local_hash = U512::from(my_peer_id2.hash());
                info!("Results of peer discovery for {:?}:", my_peer_id2);
                for n in out {
                    let other_hash = U512::from(n.hash());
                    let dist = 512 - (local_hash ^ other_hash).leading_zeros();
                    info!("* {:?} (distance bits = {:?} (lower is better))", n, dist);
                    let addr = AddrComponent::P2P(n.into_bytes()).into();
                    // TODO: this will always open a new substream instead of reusing existing ones
                    if let Err(err) =
                        swarm_controller.dial_to_handler(addr, transport.clone().with_upgrade(floodsub_upgrade.clone()))
                    {
                        info!("Error while dialing {:?}", err);
                    }
                }
                Ok(())
            })
    };

    let stdin = context.stdin().for_each(move |msg| {
        info!("> {}", msg);
        floodsub_ctl.publish(&topic, msg.into_bytes());
        Ok(())
    });

    let final_future = swarm_future
        .select(floodsub_rx)
        .map_err(|(err, _)| err)
        .map(|((), _)| ())
        .select(stdin)
        .map_err(|(err, _)| err)
        .map(|((), _)| ())
        .select(kad_poll)
        .map_err(|(err, _)| err)
        .and_then(|((), n)| n)
        .select(kad_init)
        .map_err(|(err, _)| err)
        .and_then(|((), n)| n);
    context.run(final_future);
}

enum FinalUpgrade<C> {
    Kad(KademliaProcessingFuture),
    Identify(IdentifyOutput<C>),
    FloodSub(FloodSubFuture),
}

#[derive(Clone)]
struct ConnectionUpgrader<P, R> {
    kad: KademliaUpgrade<P, R>,
    identify: IdentifyProtocolConfig,
    floodsub: FloodSubUpgrade,
}

impl<C, P, R, Pc> ConnectionUpgrade<C> for ConnectionUpgrader<P, R>
where
    C: AsyncRead + AsyncWrite + 'static,     // TODO: 'static :-/
    P: Deref<Target = Pc> + Clone + 'static, // TODO: 'static :-/
    for<'r> &'r Pc: libp2p_peerstore::Peerstore,
    R: 'static, // TODO: 'static :-/
{
    type NamesIter = ::std::vec::IntoIter<(Bytes, usize)>;
    type UpgradeIdentifier = usize;

    #[inline]
    fn protocol_names(&self) -> Self::NamesIter {
        vec![
            (Bytes::from("/ipfs/kad/1.0.0"), 0),
            (Bytes::from("/ipfs/id/1.0.0"), 1),
            (Bytes::from("/floodsub/1.0.0"), 2),
        ].into_iter()
    }

    type Output = FinalUpgrade<C>;
    type Future = Box<Future<Item = FinalUpgrade<C>, Error = IoError>>;

    fn upgrade(
        self,
        socket: C,
        id: Self::UpgradeIdentifier,
        ty: Endpoint,
        remote_addr: &Multiaddr,
    ) -> Self::Future {
        match id {
            0 => Box::new(
                self.kad
                    .upgrade(socket, (), ty, remote_addr)
                    .map(|upg| upg.into()),
            ),
            1 => Box::new(
                self.identify
                    .upgrade(socket, (), ty, remote_addr)
                    .map(|upg| upg.into()),
            ),
            2 => Box::new(
                self.floodsub
                    .upgrade(socket, (), ty, remote_addr)
                    .map(|upg| upg.into()),
            ),
            _ => unreachable!(),
        }
    }
}

impl<C> From<libp2p_kad::KademliaProcessingFuture> for FinalUpgrade<C> {
    #[inline]
    fn from(upgr: libp2p_kad::KademliaProcessingFuture) -> Self {
        FinalUpgrade::Kad(upgr)
    }
}

impl<C> From<IdentifyOutput<C>> for FinalUpgrade<C> {
    #[inline]
    fn from(upgr: IdentifyOutput<C>) -> Self {
        FinalUpgrade::Identify(upgr)
    }
}

impl<C> From<FloodSubFuture> for FinalUpgrade<C> {
    #[inline]
    fn from(upgr: FloodSubFuture) -> Self {
        FinalUpgrade::FloodSub(upgr)
    }
}

#[cfg(not(target_os = "emscripten"))]
struct PlatformSpecific {
    core: tokio_core::reactor::Core,
}
#[cfg(target_os = "emscripten")]
struct PlatformSpecific {}

#[cfg(not(target_os = "emscripten"))]
impl Default for PlatformSpecific {
    fn default() -> PlatformSpecific {
        PlatformSpecific {
            core: tokio_core::reactor::Core::new().unwrap(),
        }
    }
}
#[cfg(target_os = "emscripten")]
impl Default for PlatformSpecific {
    fn default() -> PlatformSpecific {
        PlatformSpecific {}
    }
}

#[cfg(not(target_os = "emscripten"))]
impl PlatformSpecific {
    fn build_transport(
        &self,
    ) -> libp2p_core::transport::OrTransport<
        libp2p_websocket::WsConfig<libp2p_tcp_transport::TcpConfig>,
        libp2p_tcp_transport::TcpConfig,
    > {
        let tcp = libp2p_tcp_transport::TcpConfig::new(self.core.handle());
        libp2p_websocket::WsConfig::new(tcp.clone()).or_transport(tcp)
    }

    fn stdin(&self) -> impl Stream<Item = String, Error = IoError> {
        use std::mem;

        let mut buffer = Vec::new();
        tokio_stdin::spawn_stdin_stream_unbounded()
            .map_err(|_| -> IoError { panic!() })
            .filter_map(move |msg| {
                if msg != b'\r' && msg != b'\n' {
                    buffer.push(msg);
                    return None;
                } else if buffer.is_empty() {
                    return None;
                }

                Some(String::from_utf8(mem::replace(&mut buffer, Vec::new())).unwrap())
            })
    }

    fn run<F>(mut self, future: F)
    where
        F: Future,
        F::Error: Debug,
    {
        self.core.run(future).unwrap();
    }
}
#[cfg(target_os = "emscripten")]
impl PlatformSpecific {
    // TODO: use -> impl Trait
    fn build_transport(&self) -> libp2p_websocket::BrowserWsConfig {
        stdweb::initialize();
        libp2p_websocket::BrowserWsConfig::new()
    }

    fn stdin(&self) -> impl Stream<Item = String, Error = IoError> {
        use futures::sync::mpsc;
        let (tx, rx) = mpsc::unbounded();

        let cb = move |txt: String| {
            let _ = tx.unbounded_send(txt);
        };

        js! {
            var cb = @{cb};
            document.getElementById("stdin_form")
                .addEventListener("submit", function(event) {
                    var elem = document.getElementById("stdin");
                    var txt = elem.value;
                    elem.value = "";
                    cb(txt);
                    event.preventDefault();
                });
        };

        rx.map_err(|_| -> IoError { unreachable!() })
    }

    fn run<F>(self, future: F)
    where
        F: Future + 'static,
        F::Item: Debug,
        F::Error: Debug,
    {
        use futures::{executor, Async};
        use std::sync::Mutex;

        let future_task = executor::spawn(future);

        struct Notifier<T> {
            me: Mutex<Option<executor::NotifyHandle>>,
            task: Arc<Mutex<executor::Spawn<T>>>,
        }
        let notifier = Arc::new(Notifier {
            me: Mutex::new(None),
            task: Arc::new(Mutex::new(future_task)),
        });

        let notify_handle = executor::NotifyHandle::from(notifier.clone());
        *notifier.me.lock().unwrap() = Some(notify_handle.clone());

        notify_handle.notify(0);
        stdweb::event_loop();

        unsafe impl<T> Send for Notifier<T> {}
        unsafe impl<T> Sync for Notifier<T> {}
        impl<T> executor::Notify for Notifier<T>
        where
            T: Future,
            T::Item: Debug,
            T::Error: Debug,
        {
            fn notify(&self, _: usize) {
                let task = self.task.clone();
                let me = self.me.lock().unwrap().as_ref().unwrap().clone();
                stdweb::web::set_timeout(
                    move || {
                        let val = task.lock().unwrap().poll_future_notify(&me, 0);
                        match val {
                            Ok(Async::Ready(item)) => info!("finished: {:?}", item),
                            Ok(Async::NotReady) => (),
                            Err(err) => panic!("error: {:?}", err),
                        }
                    },
                    0,
                );
            }
        }
    }
}
