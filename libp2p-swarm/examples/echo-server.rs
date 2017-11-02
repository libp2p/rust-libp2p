extern crate libp2p_swarm as swarm;
extern crate libp2p_tcp_transport as tcp;
extern crate tokio_core;

use tcp::Tcp;
use tokio_core::reactor::Core;

fn main() {
    let mut core = Core::new().unwrap();
    let tcp = Tcp::new(core.handle()).unwrap();

    let swarm = swarm::Swarm::with_details(tcp);
    swarm.listen(swarm::multiaddr::Multiaddr::new("/ip4/0.0.0.0/tcp/10333").unwrap());

    loop {
        core.turn(None);
    }
}
