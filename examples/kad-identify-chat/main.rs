//! # Chat example with Identify and Kademlia and Identify
//!
//! The kad-identify-chat example implements simple chat functionality using the `Identify` protocol
//! and the `Kademlia` DHT for peer discovery and routing. Broadcast messages are propagated using the
//! `Gossipsub` behaviour, direct messages are sent using the `RequestResponse` behaviour.
//!
//! The primary purpose of this example is to demonstrate how these behaviours interact.
//! Please see the docs on [`MyChatNetworkBehaviour`] for more details.
//! Please see `README.md` for instructions on how to run it.
//!
//! A secondary purpose of this example is to show what integration of libp2p in a complete
//! application might look like. This is similar to the file sharing example,
//! but where that example is purposely more barebones, this example is a bit more expansive and
//! uses common crates like `thiserror` and `anyhow`. It also uses the tokio runtime instead of async_std.
//!
//! Finally, it shows a way to organise your business logic using custom `EventHandler` and
//! `InstructionHandler` traits. This is by no means *the* way to do it, but it worked well
//! in the real-world application this example was derived from.
//!
//! Note how all business logic is concentrated in only four functions:
//! - `MyChatNetworkClient::handle_user_input()`: takes input from the cli and turns it into an
//! `Instruction` for the `Network`.
//! - `MyChatNetworkBehaviour::handle_instruction()`: acts on the `Instruction` using one of its
//! composed `NetworkBehaviour`s. Most likely by sending a message through the `Swarm`.
//! - `MyChatNetworkBehaviour::handle_event()`: receives events from the `Swarm` and turns them
//! into `Notification`s for `MyChatNetworkClient`.
//! - `MyChatNetworkClient::handle_notification()`: receives `Notification`s and acts on them, often
//! by formatting them and displaying them to the user.

use clap::Parser;
use tokio::sync::mpsc;

use mychat_network_client::MyChatNetworkClient;

use crate::cli::{
    MyChatCliArgs, keypair_from_peer_no, listen_address_with_peer_id, local_listen_address_from_peer_no,
};
use crate::mychat_behaviour::MyChatNetworkBehaviour;
use crate::network::network_builder::NetworkBuilder;

mod cli;
mod mychat_network_client;
mod network;
mod mychat_direct_message_protocol;
mod mychat_behaviour;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cli = MyChatCliArgs::parse();

    let (notification_tx, notification_rx) = mpsc::unbounded_channel();
    let (instruction_tx, instruction_rx) = mpsc::unbounded_channel();

    let keypair = keypair_from_peer_no(cli.peer_no);

    let behaviour = MyChatNetworkBehaviour::new(
        &keypair,
        cli.bootstrap_peer_no.map(|peer_no| vec![listen_address_with_peer_id(peer_no)]),
    )?;

    let mut network_builder =
        NetworkBuilder::new(keypair, instruction_rx, notification_tx, behaviour)?;

    // We set a custom listen address, based on the peer no
    network_builder =
        network_builder.listen_address(local_listen_address_from_peer_no(cli.peer_no));

    let network = network_builder.build()?;

    let client = MyChatNetworkClient::new(*network.peer_id(), instruction_tx, notification_rx);

    // Start network event loop
    tokio::spawn(network.run());

    // Start client event loop
    tokio::spawn(client.run()).await??;

    Ok(())
}
