use libp2p_core::{
    multiaddr::{Multiaddr, Protocol},
    transport::TransportError,
    Transport,
};

use std::io;
use std::sync::{Arc, Mutex};

use futures::channel::mpsc;
use futures::future::Ready;

#[derive(Clone)]
pub struct RelayTransportWrapper<T: Clone> {
    to_behaviour: mpsc::Sender<crate::TransportToBehaviourMsg>,
    // TODO: Can we get around the arc mutex?
    from_behaviour: Arc<Mutex<mpsc::Receiver<crate::BehaviourToTransportMsg>>>,

    inner_transport: T,
}

impl<T: Clone> RelayTransportWrapper<T> {
    pub fn new(
        t: T,
    ) -> (
        Self,
        (
            mpsc::Sender<crate::BehaviourToTransportMsg>,
            mpsc::Receiver<crate::TransportToBehaviourMsg>,
        ),
    ) {
        let (to_behaviour, from_transport) = mpsc::channel(0);
        let (to_transport, from_behaviour) = mpsc::channel(0);

        let transport = RelayTransportWrapper {
            to_behaviour,
            from_behaviour: Arc::new(Mutex::new(from_behaviour)),

            inner_transport: t,
        };

        (transport, (to_transport, from_transport))
    }
}

impl<T: Transport + Clone> Transport for RelayTransportWrapper<T> {
    type Output = <T as Transport>::Output;
    type Error = <T as Transport>::Error;
    type Listener = <T as Transport>::Listener;
    type ListenerUpgrade = <T as Transport>::ListenerUpgrade;
    type Dial = <T as Transport>::Dial;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        if !is_relay_address(&addr) {
            return self.inner_transport.listen_on(addr);
        }

        unimplemented!();
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.inner_transport.dial(addr)
    }
}

fn is_relay_address(addr: &Multiaddr) -> bool {
    addr.iter().any(|p| matches!(p, Protocol::P2pCircuit))
}
