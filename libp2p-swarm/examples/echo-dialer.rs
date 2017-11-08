extern crate libp2p_swarm as swarm;
extern crate libp2p_tcp_transport as tcp;
extern crate tokio_core;

use tcp::Tcp;
use tokio_core::reactor::Core;

fn main() {
    let mut core = Core::new().unwrap();
    let tcp = Tcp::new(core.handle()).unwrap();

    let swarm = swarm::Swarm::with_details(tcp);
    swarm.dial(swarm::multiaddr::Multiaddr::new("/ip4/127.0.0.1/tcp/4001").unwrap());

    core.run(swarm.run()).unwrap();
}
