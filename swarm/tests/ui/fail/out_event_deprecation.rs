#![deny(warnings)] // Ensure the warnings we produce surface as errors.

use libp2p_ping as ping;

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(out_event = "FooEvent", prelude = "libp2p_swarm::derive_prelude")]
struct Foo {
    ping: ping::Behaviour,
}

struct FooEvent;

impl From<ping::Event> for FooEvent {
    fn from(_: ping::Event) -> Self {
        unimplemented!()
    }
}

fn main() {

}
