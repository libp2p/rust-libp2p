use libp2p_ping as ping;

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(prelude = libp2p_swarm::derive_prelude)]
struct Foo {
    ping: ping::Behaviour,
}

fn main() {

}
