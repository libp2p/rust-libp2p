use constants::{
    ALPHA,
    KBUCKETS_TIMEOUT,
    REQUEST_TIMEOUT,
};
use futures::Future;
use futures_core::future::*;
use libp2p::CommonTransport;
use libp2p_core::{
    PeerId,
    swarm::swarm,
    Transport
};
use libp2p_kad::{
    KadSystemConfig,
    KadConnecConfig,
    KadSystem,
    kbucket::KBucketsTable
};
use libp2p_ping::Ping;
use libp2p_secio::SecioKeyPair;
use libp2p_tcp_transport::TcpConfig;
use std::time::Duration;
use tokio_current_thread;

fn init_overlay() -> Result<KadSystem, Box<Error + Send + Sync>>,
    // The following Kademlia quotes are from:
    // https://github.com/libp2p/rust-libp2p/blob/8e07c18178ac43cad3fa8974a243a98d9bc8b896/kad/src/lib.rs#L21.

    // "Build a `KadSystemConfig` and a `KadConnecConfig` object that contain the way you want the
    // Kademlia protocol to behave."
    
    let oldest_peer_id = SecioKeyPair::to_peer_id(SecioKeyPair::ed25519_generated());

    // KadSystemConfig
    // https://github.com/libp2p/rust-libp2p/blob/7507e0bfd9f11520f2d6291120f1b68d0afce80a/kad/src/high_level.rs#L36
    let kad_system_config = KadSystemConfig {
        parallelism: ALPHA,
        local_peer_id: oldest_peer_id,
        known_initial_peers: vec![],
        kbuckets_timeout: Duration::from_secs(KBUCKETS_TIMEOUT),
        request_timeout: Duration::from_secs(REQUEST_TIMEOUT),
    };

    // KadConnecConfig
    // In https://github.com/libp2p/rust-libp2p/blob/master/kad/src/kad_server.rs
    let kad_connec_config = KadConnecConfig::new();
    // Implements core::upgrade::traits::ConnectionUpgrade and .upgrade() which produces a KadConnecController,
    // but "protocol negotiation" is required before upgrading. See also core::Readme#Connection_upgrades:

    // > Once a socket has been opened with a remote through a `Transport`, it can be *upgraded*. This
    // consists in negotiating a protocol with the remote (through `multistream-select`), and applying
    // that protocol on the socket.

    // > A potential connection upgrade is represented with the `ConnectionUpgrade` trait. The trait
    // consists in a protocol name plus a method that turns the socket into an `Output` object whose
    // nature and type is specific to each upgrade.

    // > There exists three kinds of connection upgrades: middlewares, muxers, and actual protocols.

    // A description of each ensues. KadConnecConfig doesn't seem to match any of these.

    // > The output of a middleware connection upgrade implements the `AsyncRead` and `AsyncWrite`
    // traits, just like sockets do. 

    // Doesn't seem to be so. Although the ConnectionUpgrade trait implements the `AsyncRead` and `AsyncWrite`
    // traits, it's output doesn't seem to in kad_server.
    // impl<C, Maf> ConnectionUpgrade<C, Maf> for KadConnecConfig
    // where
    //     C: AsyncRead + AsyncWrite + 'static, // TODO: 'static :-/
    // {
    //     type Output = (
    //         KadConnecController,
    //         Box<Stream<Item = KadIncomingRequest, Error = IoError>>,
    //     );

    // On muxing: If the output of the connection upgrade instead implements the `StreamMuxer` and `Clone`
    // traits, then you can turn the `UpgradedNode` struct into a `ConnectionReuse` struct by calling
    // `ConnectionReuse::from(upgraded_node)`.

    // Again, not applicable.

    // "Create a swarm that upgrades incoming connections with the `KadConnecConfig`.

    // > *Actual protocols* work the same way as middlewares, except that their `Output` doesn't
    // implement the `AsyncRead` and `AsyncWrite` traits. This means that that the return value of
    // `with_upgrade` does **not** implement the `Transport` trait and thus cannot be used as a
    // transport.

    // > However the `UpgradedNode` struct returned by `with_upgrade` still provides methods named
    // `dial`, `listen_on`, and `next_incoming`, which will yield you a `Future` or a `Stream`,
    // which you can use to obtain the `Output`. This `Output` can then be used in a protocol-specific
    // way to use the protocol.

    // As mentioned, KadConnecConfig's Output doesn't implement the `AsyncRead` and `AsyncWrite` traits,
    // but it also doesn't implement `with_upgrade`, just `upgrade`.

    // So it isn't clear how to put a `KadConnecConfig` into a swarm, by first implementing a Transport.

    // Could use `PlainTextConfig` or `secio`. For Ethereum a security layer is desirable, so let's try to use
    // secio.
    // Note: PlainTextConfig: core/src/upgrade/plaintext.rs
    // 

    let transport = CommonTransport::new()
        .with_upgrade({
            let upgrade = SecioConfig {
                // See the documentation of `SecioKeyPair`.
                key: ed25519_generated().unwrap(),
            }.unwrap();

            upgrade::map(upgrade, |out: SecioOutput<_>| out.stream).unwrap()
        });

    let mut multiaddr = address
        .parse::<Multiaddr>()
        .expect("failed to parse hard-coded multiaddr");

    // Done to implement the transport trait in order to put it in a swarm.
    // fn upgrade(self, incoming: C, id: (), endpoint: Endpoint, addr: Maf) -> Self::Future
    // C: AsyncRead + AsyncWrite + 'static, Transport and wrappers around it implements this.
    // libp2p_core::upgrade::traits::{ConnectionUpgrade, Endpoint}
    // /// Type of connection for the upgrade.
    // #[derive(Debug, Copy, Clone, PartialEq, Eq)]
    // pub enum Endpoint {
    //     /// The socket comes from a dialer.
    //     Dialer,
    //     /// The socket comes from a listener.
    //     Listener,
    // }
    // Assume to choose a dialer for now as this is creating the kad_connec_config.
    // pub trait ConnectionUpgrade<C, TAddrFut> {
    //  ...
    // /// Type of the future that will resolve to the remote's multiaddr.
    let kad_connec_config_transport_dialer = kad_connec_config
        .upgrade(transport, (), Dialer, Future::multiaddr).unwrap();

    let (swarm_controller, swarm_future) = swarm(kad_connec_config_transport,
            Ping, |(mut pinger, service), client_addr| {
        pinger.ping().map_err(|_| panic!())
            .select(service).map_err(|_| panic!())
            .map(|_| ())
    });

    // The `swarm_controller` can then be used to do some operations.
    // swarm_controller.listen_on("/ip4/0.0.0.0/tcp/0".parse().unwrap());

    // "Build a `KadSystem` from the `KadSystemConfig`. This requires passing a closure that provides
    // the Kademlia controller of a peer."
    // FMI see https://github.com/libp2p/rust-libp2p/blob/master/kad/src/high_level.rs
    // let access = |peer_id: &PeerId| {
    //     ok::
    // };

    //     pub fn start<'a, F, Fut>(config: KadSystemConfig<impl Iterator<Item = PeerId>>, access: F)
    //     -> (KadSystem, impl Future<Item = (), Error = IoError> + 'a)
    //     where F: FnMut(&PeerId) -> Fut + Clone + 'a,
    //         Fut: IntoFuture<Item = KadConnecController, Error = IoError>  + 'a,
    // {
    //     let system = KadSystem::without_init(config);
    //     let init_future = system.perform_initialization(access);
    //     (system, init_future)
    // }

    // TODO: complete initialization of KadSystem,
    // let kbuckets_table = KBucketsTable {
        
    // };

    // let kad_system = KadSystem {
    //     kbuckets : 
    // }.start(kad_system_config, kad_peer_controller(oldest_peer_id));
    // Ok(kad_system)

    // pub struct KBucketsTable<Id, Val> {
    // my_id: Id,
    // tables: Vec<Mutex<KBucket<Id, Val>>>,
    // // The timeout when pinging the first node after which we consider that it no longer responds.
    // ping_timeout: Duration,
    // }
    
    // Runs until everything is finished.
    tokio_current_thread::block_on_all(swarm_future).unwrap();
}

// "You can perform queries using the `KadSystem`." TODO: Test

// Join overlay
// 
// Obtain initial contact nodes via rendevous with DHT provider records.

// "Send a GETNODE message in order to obtain an up-to-date view of the overlay from the passive list of a 
// subscribed node regardless of age of Provider records.



// Once an up-to-date passive view of the overlay has been
// obtained, the node proceeds to join.

// In order to join, it picks `C_rand` nodes at random and sends
// `JOIN` messages to them with some initial TTL set as a design parameter.

// The `JOIN` message propagates with a random walk until a node is willing
// to accept it or the TTL expires. Upon receiving a `JOIN` message, a node Q
// evaluates it with the following criteria:
// - Q tries to open a connection to P. If the connection cannot be opened (e.g. because of NAT),
//   then it checks the TTL of the message.
//   If it is 0, the request is dropped, otherwise Q decrements the TTL and forwards
//   the message to a random node in its active list.
// - If the TTL of the request is 0 or if the size of Q's active list is less than `A`,
//   it accepts the join, adds P to its active list and sends a `NEIGHBOR` message.
// - Otherwise it decrements the TTL and forwards the message to a random node
//   in its active list.

// When Q accepts P as a new neighbor, it also sends a `FORWARDJOIN`
// message to a random node in its active list. The `FORWARDJOIN`
// propagates with a random walk until its TTL is 0, while being added to
// the passive list of the receiving nodes.

// If P fails to join because of connectivity issues, it decrements the
// TTL and tries another starting node. This is repeated until a TTL of zero
// reuses the connection in the case of NATed hosts.

// Once the first links have been established, P then needs to increase
// its active list size to `A` by connecting to more nodes.  This is
// accomplished by ordering the subscriber list by RTT and picking the
// nearest nodes and sending `NEIGHBOR` requests.  The neighbor requests
// may be accepted by `NEIGHBOR` message and rejected by a `DISCONNECT`
// message.

// Upon receiving a `NEIGHBOR` request a node Q evaluates it with the
// following criteria:
// - If the size of Q's active list is less than A, it accepts the new
//   node.
// - If P does not have enough active links (less than `C_rand`, as specified in the message),
//   it accepts P as a random neighbor.
// - Otherwise Q takes an RTT measurement to P.
//   If it's closer than any near neighbors by a factor of alpha, then
//   it evicts the near neighbor if it has enough active links and accepts
//   P as a new near neighbor.
// - Otherwise the request is rejected.

// Note that during joins, the size of the active list for some nodes may
// end up being larger than `A`. Similarly, P may end up with fewer links
// than `A` after an initial join. This follows [3] and tries to minimize
// fluttering in joins, leaving the active list pruning for the
// stabilization period of the protocol.

#[cfg(test)]
mod tests {
    use kad::KadSystem;

    #[test]
    fn get_local_peer_id() {
        let peer_id = KadSystem.local_peer_id();
        assert_eq!(sample_peer_id, peer_id)
    }
}