use libp2p_core::{multiaddr::Multiaddr, Transport, transport::TransportError};

use std::io;

use futures::future::Ready;

#[derive(Clone)]
pub struct RelayTransportWrapper<T: Clone> {
    inner_transport: T,
}

impl<T: Clone> RelayTransportWrapper<T> {
    pub fn new(t: T) -> Self {
        RelayTransportWrapper {
            inner_transport: t,
        }
    }
}

impl<T: Transport + Clone> Transport for RelayTransportWrapper<T> {
    type Output = <T as Transport>::Output;
    type Error = <T as Transport>::Error;
    type Listener = <T as Transport>::Listener;
    type ListenerUpgrade = <T as Transport>::ListenerUpgrade;
    type Dial = <T as Transport>::Dial;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        self.inner_transport.listen_on(addr)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        self.inner_transport.dial(addr)
    }
}
