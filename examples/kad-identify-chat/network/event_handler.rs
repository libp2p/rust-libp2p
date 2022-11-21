use async_trait::async_trait;
use libp2p::swarm::{ConnectionHandler, IntoConnectionHandler, NetworkBehaviour, SwarmEvent};
use tokio::sync::mpsc;
use crate::network::Notification;


#[async_trait]
pub trait EventHandler
where
    Self: NetworkBehaviour,
{
    async fn handle_event(
        &mut self,
        notification_tx: &mpsc::UnboundedSender<Notification>,
        event: SwarmEvent<
            Self::OutEvent,
            <<Self::ConnectionHandler as IntoConnectionHandler>::Handler as ConnectionHandler>::Error,
        >,
    );
}
