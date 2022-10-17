use libp2p::core::transport::ListenerId;
use libp2p::swarm::behaviour::FromSwarm;
use libp2p::swarm::dummy;
use libp2p_swarm::behaviour::{ConnectionEstablished, ConnectionClosed, AddressChange, DialFailure, ListenFailure, NewListener, NewListenAddr, ExpiredListenAddr, ListenerError, ListenerClosed, NewExternalAddr, ExpiredExternalAddr};

#[test]
// test to break compilation everytime a variant changes,
// forcing us to revisit each implementation
fn swarm_event_variants() {
    let event: FromSwarm<'_, dummy::ConnectionHandler> =
        FromSwarm::ListenerClosed(libp2p_swarm::behaviour::ListenerClosed {
            listener_id: ListenerId::new(),
            reason: Ok(()),
        });
    match event {
        FromSwarm::ConnectionEstablished( ConnectionEstablished {
            peer_id: _,
            connection_id: _,
            endpoint: _,
            failed_addresses: _,
            other_established: _,
        }) => {}
        FromSwarm::ConnectionClosed(ConnectionClosed {
            peer_id: _,
            connection_id: _,
            endpoint: _,
            handler: _,
            remaining_established: _,
        }) => {}
        FromSwarm::AddressChange(AddressChange {
            peer_id: _,
            connection_id: _,
            old: _,
            new: _,
        }) => {}
        FromSwarm::DialFailure(DialFailure {
            peer_id: _,
            handler: _,
            error: _,
        }) => {}
        FromSwarm::ListenFailure(ListenFailure {
            local_addr: _,
            send_back_addr: _,
            handler: _,
        }) => {}
        FromSwarm::NewListener(NewListener { listener_id: _ }) => {}
        FromSwarm::NewListenAddr(NewListenAddr {
            listener_id: _,
            addr: _,
        }) => {}
        FromSwarm::ExpiredListenAddr(ExpiredListenAddr {
            listener_id: _,
            addr: _,
        }) => {}
        FromSwarm::ListenerError(ListenerError {
            listener_id: _,
            err: _,
        }) => {}
        FromSwarm::ListenerClosed(ListenerClosed {
            listener_id: _,
            reason: _,
        }) => {}
        FromSwarm::NewExternalAddr(NewExternalAddr { addr: _ }) => {}
        FromSwarm::ExpiredExternalAddr(ExpiredExternalAddr { addr: _ }) => {}
    }
}
