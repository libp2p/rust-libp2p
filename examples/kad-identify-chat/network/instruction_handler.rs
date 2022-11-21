use async_trait::async_trait;
use libp2p::swarm::NetworkBehaviour;
use tokio::sync::mpsc;
use crate::network::{Instruction, Notification};


#[async_trait]
pub trait InstructionHandler
where
    Self: NetworkBehaviour,
{
    async fn handle_instruction(
        &mut self,
        notification_tx: &mpsc::UnboundedSender<Notification>,
        instruction: Instruction,
    );
}
