use std::{
    collections::{HashMap, VecDeque},
    convert::Infallible,
    error::Error,
    sync::atomic::AtomicUsize,
    task::{Context, Poll, Waker},
    time::Duration,
};

use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use libp2p::{
    Multiaddr, PeerId,
    core::{Endpoint, transport::PortUse},
    noise, ping,
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, OutboundAddresses, SwarmEvent,
        THandler, THandlerInEvent, THandlerOutEvent, ToSwarm, dial_opts::DialOpts, dummy,
    },
    tcp, yamux,
};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use tracing_subscriber::EnvFilter;

/// A [`NetworkBehaviour`] that resolves dial addresses from an SQLite database.
struct SqliteAddressBook {
    pool: SqlitePool,
    queue: VecDeque<ToSwarm<<Self as NetworkBehaviour>::ToSwarm, THandlerInEvent<Self>>>,
    pending_addr: HashMap<Ticket, (PeerId, Multiaddr)>,
    pending_tasks: FuturesUnordered<BoxFuture<'static, (Ticket, sqlx::Result<()>)>>,
    waker: Option<Waker>,
}

static TICKET: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct Ticket(usize);

impl Ticket {
    fn next() -> Self {
        Ticket(TICKET.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
    }
}

impl SqliteAddressBook {
    fn put(&mut self, peer: PeerId, address: &Multiaddr) -> Ticket {
        let ticket = Ticket::next();
        let addr = address.clone();
        let pool = self.pool.clone();
        let fut = async move {
            let ret = sqlx::query("INSERT INTO peer_addresses (peer, address) VALUES (?, ?)")
                .bind(peer.to_string())
                .bind(addr.to_string())
                .execute(&pool)
                .await
                .map(|_| ());
            (ticket, ret)
        }
        .boxed();
        self.pending_addr.insert(ticket, (peer, address.clone()));
        self.pending_tasks.push(fut);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
        ticket
    }

    // fn is_pending(&self, ticket: Ticket) -> bool {
    //     self.pending_addr.contains_key(&ticket)
    // }
}

impl NetworkBehaviour for SqliteAddressBook {
    type ConnectionHandler = dummy::ConnectionHandler;
    type ToSwarm = Infallible;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _: ConnectionId,
        maybe_peer: Option<PeerId>,
        _: &[Multiaddr],
        _: Endpoint,
    ) -> OutboundAddresses {
        let Some(peer) = maybe_peer else {
            return OutboundAddresses::Ready(Ok(vec![]));
        };

        let pool = self.pool.clone();
        OutboundAddresses::Pending(Box::pin(async move {
            let rows: Vec<String> =
                sqlx::query_scalar("SELECT address FROM peer_addresses WHERE peer = ?")
                    .bind(peer.to_string())
                    .fetch_all(&pool)
                    .await
                    .map_err(ConnectionDenied::new)?;

            Ok(rows
                .iter()
                .filter_map(|address| address.parse().ok())
                .collect())
        }))
    }

    fn on_swarm_event(&mut self, _: FromSwarm) {}

    fn on_connection_handler_event(
        &mut self,
        _: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match event {}
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        while let Poll::Ready(Some((ticket, result))) = self.pending_tasks.poll_next_unpin(cx) {
            let (peer_id, address) = self.pending_addr.remove(&ticket).expect("ticket valid");
            if result.is_ok() {
                self.queue
                    .push_back(ToSwarm::NewExternalAddrOfPeer { peer_id, address });
            }
        }

        if let Some(event) = self.queue.pop_front() {
            return Poll::Ready(event);
        }

        self.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

#[derive(NetworkBehaviour)]
struct DialerBehaviour {
    address_book: SqliteAddressBook,
    ping: ping::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init();

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    sqlx::query("CREATE TABLE peer_addresses (peer TEXT NOT NULL, address TEXT NOT NULL)")
        .execute(&pool)
        .await?;

    let mut address_book = SqliteAddressBook {
        pool,
        queue: VecDeque::new(),
        pending_addr: HashMap::new(),
        pending_tasks: FuturesUnordered::new(),
        waker: None,
    };

    let mut responder = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|_| ping::Behaviour::default())?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(30)))
        .build();
    responder.listen_on("/ip4/127.0.0.1/tcp/0".parse()?)?;

    let responder_peer = *responder.local_peer_id();
    let responder_addr = loop {
        if let SwarmEvent::NewListenAddr { address, .. } = responder.select_next_some().await {
            break address;
        }
    };

    println!("responder {responder_peer} listening on {responder_addr}");

    address_book.put(responder_peer, &responder_addr);
    println!("queued the responder address");

    let mut dialer = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(move |_| DialerBehaviour {
            address_book,
            ping: ping::Behaviour::default(),
        })?
        .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(30)))
        .build();

    dialer.dial(
        DialOpts::peer_id(responder_peer)
            .addresses(vec![])
            .extend_addresses_through_behaviour()
            .build(),
    )?;

    let timer = tokio::time::sleep(Duration::from_secs(10));
    futures::pin_mut!(timer);
    loop {
        tokio::select! {
            _ = &mut timer => {
                break;
            }
            _ = responder.select_next_some() => {}
            event = dialer.select_next_some() => match event {
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    println!("connected to {peer_id}");
                }
                SwarmEvent::Behaviour(DialerBehaviourEvent::Ping(ping::Event {
                    peer, result, ..
                })) => {
                    println!("ping to {peer}: {result:?}");
                }
                SwarmEvent::OutgoingConnectionError { error, .. } => {
                    println!("dialing {responder_peer} failed: {error}");
                }
                SwarmEvent::NewExternalAddrOfPeer { peer_id, address } => {
                    println!("{peer_id} is now reachable at {address}");
                }
                _ => {}
            }
        }
    }
    Ok(())
}
