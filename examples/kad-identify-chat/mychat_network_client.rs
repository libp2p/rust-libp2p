use std::fmt::Debug;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Context;
use libp2p::PeerId;
use tokio::io::{self, AsyncBufReadExt, BufReader, Lines, Stdin};
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::network::{Address, Instruction, Notification};

use crate::cli::keypair_from_peer_no;
use serde::{Serialize, Deserialize};

/// The `NetworkClient` ireads user input from standard input and transforms it
/// into `Instruction`s. It then sends the `Instruction` on the mpsc channel that the `Network`
/// listens to.
///
/// It receives `Notification`s from the `Network` on the notification channel.
pub struct MyChatNetworkClient {
    peer_id: PeerId,
    instruction_tx: mpsc::UnboundedSender<Instruction>,
    notification_rx: mpsc::UnboundedReceiver<Notification>,
    stdin: Lines<BufReader<Stdin>>,
}

impl MyChatNetworkClient {
    pub fn new(
        peer_id: PeerId,
        instruction_tx: mpsc::UnboundedSender<Instruction>,
        notification_rx: mpsc::UnboundedReceiver<Notification>,
    ) -> MyChatNetworkClient {
        MyChatNetworkClient {
            peer_id,
            instruction_tx,
            notification_rx,
            stdin: BufReader::new(io::stdin()).lines(),
        }
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        log::info!("Start network client event loop.");
        loop {
            tokio::select! {
                Ok(Some(line)) = self.stdin.next_line() => {
                    Self::handle_user_input(self.peer_id, self.instruction_tx.clone(), &line).await?
                }
                Some(notification) = self.notification_rx.recv() =>  {
                     Self::handle_notification(self.peer_id, self.instruction_tx.clone(), notification).await?
                }
                else => {
                    log::info!("Both stdin and notification channel closed. Ending client");
                    break Ok(())

                }
            }
        }
    }

    async fn handle_user_input(
        local_peer_id: PeerId,
        instruction_tx: mpsc::UnboundedSender<Instruction>,
        input: &str,
    ) -> anyhow::Result<()> {
        if input == "ls" {
            Self::request_peer_list(instruction_tx.clone()).await?
        } else if input.starts_with("dm") {
            let split: Vec<&str> = input.split_whitespace().collect();
            let message = Message::DirectMessage {
                source: local_peer_id,
                data: split[2..].join(" ")
            };

            Self::send_dm_to_peer_no(
                instruction_tx.clone(),
                u8::from_str(split[1]).context(format!(
                    "Can't send DM: peer no is not an integer: {}",
                    split[1]
                ))?,
                message,
            )
            .await?;
        } else {
            Self::send_broadcast(local_peer_id, instruction_tx.clone()).await?
        }

        Ok(())
    }

    async fn handle_notification(
        local_peer_id: PeerId,
        instruction_tx: mpsc::UnboundedSender<Instruction>,
        notification: Notification,
    ) -> anyhow::Result<()> {
        match notification {
            Notification::Data(payload) => {
                let message: Message = bincode::deserialize(&payload)?;
                match message {
                    Message::Ack { source } => {
                        log::info!("Received ACK from peer {source} on broadcast");
                    }
                    Message::Broadcast { source } => {
                        log::info!("Received BROADCAST message from peer {source}");
                        tokio::spawn(async move {
                            sleep(Duration::from_secs(1)).await;

                            let _ = Self::send_dm(
                                instruction_tx,
                                source,
                                Message::Ack {
                                    source: local_peer_id,
                                },
                            )
                            .await;
                        });
                    }
                    Message::DirectMessage { source, data } => {
                        log::info!("Received DM from peer {source}: {data}");
                    }
                }
            }
            Notification::Err(error) => {
                log::error!("Received error from network: {error}")
            }
            Notification::PeerList(peer_list) => {
                log::info!("Peer list: {peer_list:?}")
            }
        }

        Ok(())
    }

    async fn request_peer_list(
        instruction_tx: mpsc::UnboundedSender<Instruction>,
    ) -> anyhow::Result<()> {
        let instruction = Instruction::PeerList;
        Ok(instruction_tx.send(instruction)?)
    }

    async fn send_dm_to_peer_no(
        instruction_tx: mpsc::UnboundedSender<Instruction>,
        to_peer_no: u8,
        message: Message,
    ) -> anyhow::Result<()> {
        Self::send_dm(
            instruction_tx,
            keypair_from_peer_no(to_peer_no).public().to_peer_id(),
            message,
        )
        .await
    }

    async fn send_dm(
        instruction_tx: mpsc::UnboundedSender<Instruction>,
        to_peer_id: PeerId,
        message: Message,
    ) -> anyhow::Result<()> {
        let instruction = Instruction::Send {
            destination: Address::DirectMessage(to_peer_id.clone()),
            message: bincode::serialize(&message)?.into(),
        };
        instruction_tx.send(instruction)?;

        log::info!(
            "Direct message sent to peer: '{}': {:?}",
            to_peer_id,
            message
        );

        Ok(())
    }

    async fn send_broadcast(
        from_peer_id: PeerId,
        instruction_tx: mpsc::UnboundedSender<Instruction>,
    ) -> anyhow::Result<()> {
        let instruction = Instruction::Send {
            destination: Address::Broadcast,
            message: bincode::serialize(&Message::Broadcast {
                source: from_peer_id,
            })?
            .into(),
        };

        instruction_tx.send(instruction)?;

        log::info!("Broadcast sent to Network");

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
enum Message {
    Ack { source: PeerId },
    Broadcast { source: PeerId },
    DirectMessage { source: PeerId, data: String },
}
