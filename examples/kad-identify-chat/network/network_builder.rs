use std::io;

use thiserror::Error;
use tokio::sync::mpsc;

use identity::Keypair;
use libp2p::{identity, mplex, multiaddr, Multiaddr, noise, PeerId, Transport, TransportError};
use libp2p::core::muxing::StreamMuxerBox;
use libp2p::core::transport;
use libp2p::core::transport::upgrade;
use libp2p::noise::NoiseError;
use libp2p::swarm::{NetworkBehaviour, SwarmBuilder};
use libp2p_tcp::tokio::Tcp;

use crate::network::{Instruction, Network, Notification};
use crate::network::event_handler::EventHandler;
use crate::network::instruction_handler::InstructionHandler;

pub struct NetworkBuilder<TBehaviour> {
    keypair: Keypair,
    instruction_rx: mpsc::UnboundedReceiver<Instruction>,
    notification_tx: mpsc::UnboundedSender<Notification>,
    transport: transport::Boxed<(PeerId, StreamMuxerBox)>,
    listen_address: Multiaddr,
    behaviour: TBehaviour,
}

#[allow(dead_code)]
impl<TBehaviour> NetworkBuilder<TBehaviour>
    where
        TBehaviour: NetworkBehaviour + EventHandler + InstructionHandler,
{
    pub fn new(
        keypair: Keypair,
        instruction_rx: mpsc::UnboundedReceiver<Instruction>,
        notification_tx: mpsc::UnboundedSender<Notification>,
        behaviour: TBehaviour,
    ) -> Result<NetworkBuilder<TBehaviour>, NetworkBuilderError> {
        Ok(NetworkBuilder {
            transport: Self::default_transport(&keypair)?,
            listen_address: Self::default_listen_address()?,
            keypair,
            instruction_rx,
            notification_tx,
            behaviour,
        })
    }

    pub fn listen_address(mut self, address: Multiaddr) -> Self {
        self.listen_address = address;
        self
    }

    pub fn transport(mut self, transport: transport::Boxed<(PeerId, StreamMuxerBox)>) -> Self {
        self.transport = transport;
        self
    }

    /// Listen on all interfaces, on a random port
    fn default_listen_address() -> Result<Multiaddr, NetworkBuilderError> {
        Ok("/ip4/0.0.0.0/tcp/0".parse()?)
    }

    fn default_transport(
        keypair: &Keypair,
    ) -> Result<transport::Boxed<(PeerId, StreamMuxerBox)>, NetworkBuilderError> {
        // Create a keypair for authenticated encryption of the transport.
        let noise_keys = noise::Keypair::<noise::X25519Spec>::new().into_authentic(keypair)?;

        Ok(
            libp2p_tcp::Transport::<Tcp>::new(libp2p_tcp::Config::default().nodelay(true))
                .upgrade(upgrade::Version::V1)
                .authenticate(noise::NoiseConfig::xx(noise_keys).into_authenticated())
                .multiplex(mplex::MplexConfig::new())
                .boxed(),
        )
    }

    pub fn build(self) -> Result<Network<TBehaviour>, NetworkBuilderError> {
        let mut swarm = {
            SwarmBuilder::with_executor(
                self.transport,
                self.behaviour,
                PeerId::from(self.keypair.public()),
                Box::new(|fut| {
                    // We want the connection background tasks to be spawned
                    // onto the tokio runtime.
                    tokio::spawn(fut);
                }),
            )
                .build()
        };
        swarm.listen_on(self.listen_address)?;

        log::info!("Local PeerId: {}", swarm.local_peer_id());

        Ok(Network::new(
            self.instruction_rx,
            self.notification_tx,
            swarm,
        ))
    }
}

#[derive(Debug, Error)]
pub enum NetworkBuilderError {
    #[error(transparent)]
    BuildError(#[from] TransportError<io::Error>),
    #[error(transparent)]
    TransportError(#[from] NoiseError),
    #[error(transparent)]
    ListenAddressError(#[from] multiaddr::Error),
}
