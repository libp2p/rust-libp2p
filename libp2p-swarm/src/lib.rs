//! Transport and I/O primitives for libp2p.

extern crate bytes;
#[macro_use]
extern crate futures;
extern crate multistream_select;
extern crate tokio_io;

use std::io::Error as IoError;
use std::mem;
use std::sync::{Arc, Mutex};
use bytes::Bytes;
use futures::{IntoFuture, Future, Sink, Stream};
use futures::future::join_all;
use multiaddr::Multiaddr;
use tokio_io::{AsyncRead, AsyncWrite};

/// Multi-address re-export.
pub extern crate multiaddr;

mod connec_upgrade;
mod socket;
pub mod transport;

pub use self::connec_upgrade::{AbstractConnUpgr, ConnectionUpgrade, PlainText};
pub use self::socket::{ProtocolId, PeerId, Socket, Conn};
pub use self::transport::Transport;

/// A `Swarm` is the core of libp2p. It is what links the various components (transports, security,
/// multiplex, discovery, etc.) together.
pub struct Swarm<T> {       // TODO: provide a default for T
    transports: Mutex<T>,
    listeners: Mutex<Vec<Box<Future<Item = (), Error = IoError>>>>,
    dialers: Mutex<Vec<Box<Future<Item = (), Error = IoError>>>>,
    security_protocols: Vec<(Bytes, fn(&Bytes,&Bytes)->bool, Arc<AbstractConnUpgr>)>,
}

// TODO: work in progress
impl<T> Swarm<T> {
    /// Builds a new swarm.
    #[inline]
    pub fn new() -> Self
        where T: Default
    {
        Self::with_details(Default::default())
    }

    /// Adds a connection upgrade protocol.
    ///
    /// This function registers a new protocol that can be used to upgrade a connection.
    // TODO: does the dialer try this protocol every time?
    pub fn add_connection_upgrade<C>(&mut self, protocol_name: Bytes, upgrade: C)
        where C: ConnectionUpgrade<Box<AsyncReadWrite>> + 'static
    {
        self.security_protocols.insert(0, (protocol_name, <Bytes as PartialEq>::eq as fn(&Bytes, &Bytes)->bool, Arc::new(upgrade)));
    }

    /// Builds a swarm and lets you initialize the transports list.
    #[inline]
    pub fn with_details(transports: T) -> Self {
        let security_protocols = vec![
            (Bytes::from("/plaintext/1.0.0"), <Bytes as PartialEq>::eq as fn(&Bytes, &Bytes)->bool,
                                              Arc::new(PlainText) as Arc<_>)
        ];

        Swarm {
            transports: Mutex::new(transports),
            listeners: Mutex::new(Vec::new()),      // TODO: with_capacity with the number of multiaddrs
            dialers: Mutex::new(Vec::new()),
            security_protocols: security_protocols,
        }
    }

    // TODO: get more thoughts into this API
    pub fn run(self) -> Box<Future<Item = (), Error = IoError>> {
        let listeners = join_all(mem::replace(&mut *self.listeners.lock().unwrap(), Vec::new())).map(|_| ());
        let dialers = join_all(mem::replace(&mut *self.dialers.lock().unwrap(), Vec::new())).map(|_| ());
        Box::new(listeners.join(dialers).map(|_| ()))
    }
}

impl<T> Swarm<T> where T: Transport, T::Dial: 'static, T::RawConn: 'static {        // TODO: 'static?
    /// Tries to dial on all added multiaddresses of the peer info.
    // TODO: produce a future that is signalled when listening starts, like the JS API
    #[inline]
    pub fn dial(&self, addr: Multiaddr) {
        let security_protocols = self.security_protocols.clone().into_iter();

        let multiplex_protocols = vec![
            (Bytes::from("/multiplex/1.0.0"), <Bytes as PartialEq>::eq, 0)
        ].into_iter();

        let finished = self.transports.lock().unwrap()
            .dial(addr).unwrap()       // TODO: don't panic
            .into_future()
            .map_err(|_| panic!())       // TODO:
            // Negociate the security protocol.
            .and_then(move |connection| {
                multistream_select::dialer_select_proto(connection, security_protocols.clone())
                    .map_err(|err| panic!("{:?}", err))      // TODO:
            })
            .and_then(|(chosen_sec, connection)| {
                Ok(AbstractConnUpgr::upgrade(&*chosen_sec, Box::new(connection) as Box<_>))
            })
            // Negociate the multiplex.
            .and_then(move |connection| {
                multistream_select::dialer_select_proto(connection, multiplex_protocols.clone())
                    .map_err(|err| panic!("{:?}", err))      // TODO:
            })
            .and_then(|(chosen_multiplex, connection)| {
                Ok(())
            })
            .map_err(|_| panic!());     // TODO:

        self.dialers.lock().unwrap().push(Box::new(finished));
    }
}

impl<T> Swarm<T> where T: Transport, T::Listener: 'static, T::RawConn: 'static {        // TODO: 'static?
    /// Start listening on the multiaddr.
    // TODO: should take multiple multiaddrs (through a peerinfo).
    // TODO: produce a future that is signalled when listening starts, like the JS API
    #[inline]
    pub fn listen(&self, addr: Multiaddr) {
        let security_protocols = self.security_protocols.clone().into_iter();

        let multiplex_protocols = vec![
            (Bytes::from("/multiplex/1.0.0"), <Bytes as PartialEq>::eq, 0)
        ].into_iter();

        let finished = self.transports.lock().unwrap()
            .listen_on(addr).unwrap()       // TODO: don't panic
            .map_err(|_| panic!())       // TODO:
            // Negociate the security protocol.
            .and_then(move |connection| {
                multistream_select::listener_select_proto(connection, security_protocols.clone())
                    .map_err(|err| panic!("{:?}", err))      // TODO:
            })
            .and_then(|(chosen_sec, connection)| {
                Ok(AbstractConnUpgr::upgrade(&*chosen_sec, Box::new(connection) as Box<_>))
            })
            // Negociate the multiplex.
            .and_then(move |connection| {
                multistream_select::listener_select_proto(connection, multiplex_protocols.clone())
                    .map_err(|err| panic!("{:?}", err))      // TODO:
            })
            .and_then(|(chosen_multiplex, connection)| {
                Ok(())
            })
            .for_each(|_| Ok(()))
            .map_err(|_| panic!());     // TODO:

        self.listeners.lock().unwrap().push(Box::new(finished));
    }
}

pub trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite {}
