
use web_sys::WebTransport;
use libp2p_core::Transport;

pub struct WebTransportTransport;

impl Transport for WebTransportTransport {
    type Output = ();

    type Error= ();

    type ListenerUpgrade= ();

    type Dial= ();

    fn listen_on(&mut self, addr: libp2p_core::Multiaddr) -> Result<libp2p_core::transport::ListenerId, libp2p_core::transport::TransportError<Self::Error>> {
        todo!()
    }

    fn remove_listener(&mut self, id: libp2p_core::transport::ListenerId) -> bool {
        todo!()
    }

    fn dial(&mut self, addr: libp2p_core::Multiaddr) -> Result<Self::Dial, libp2p_core::transport::TransportError<Self::Error>> {
        todo!()
    }

    fn dial_as_listener(
        &mut self,
        addr: libp2p_core::Multiaddr,
    ) -> Result<Self::Dial, libp2p_core::transport::TransportError<Self::Error>> {
        todo!()
    }

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p_core::transport::TransportEvent<Self::ListenerUpgrade, Self::Error>> {
        todo!()
    }

    fn address_translation(&self, listen: &libp2p_core::Multiaddr, observed: &libp2p_core::Multiaddr) -> Option<libp2p_core::Multiaddr> {
        todo!()
    }
}
