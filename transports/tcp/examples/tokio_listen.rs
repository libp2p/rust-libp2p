// Copyright 2022 Protocol Labs.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use anyhow::Result;
use futures::prelude::*;
use libp2p_core::transport::TransportEvent;
use libp2p_core::Transport;
use libp2p_tcp::tokio::TcpStream;
use libp2p_tcp::GenTcpConfig;
use tokio_crate as tokio;

#[tokio::main]
async fn main() -> Result<()> {
    let mut transport = libp2p_tcp::TokioTcpTransport::new(GenTcpConfig::new()).boxed();

    transport.listen_on("/ip4/0.0.0.0/tcp/9999".parse()?)?;

    loop {
        match transport.select_next_some().await {
            TransportEvent::NewAddress { listen_addr, .. } => {
                eprintln!("Listening on {listen_addr}");
            }
            TransportEvent::AddressExpired { listen_addr, .. } => {
                eprintln!("No longer listening on {listen_addr}");
            }
            TransportEvent::Incoming {
                send_back_addr,
                upgrade,
                ..
            } => {
                eprintln!("Incoming connection from {send_back_addr}");

                let task = tokio::spawn(async move {
                    let stream = upgrade.await?;
                    ping_pong(stream).await?;

                    anyhow::Ok(())
                });
                tokio::spawn(async move {
                    if let Err(e) = task.await {
                        eprintln!("Task failed: {e}")
                    }
                });
            }
            TransportEvent::ListenerClosed { .. } => {}
            TransportEvent::ListenerError { error, .. } => {
                eprintln!("Listener encountered an error: {error}");
            }
        }
    }
}

async fn ping_pong(mut stream: TcpStream) -> Result<()> {
    let addr = stream.0.peer_addr()?;

    loop {
        let mut buffer = [0u8; 4];
        stream.read_exact(&mut buffer).await?;

        anyhow::ensure!(&buffer == b"PING");

        eprintln!("Received PING from {addr}");

        stream.write_all(b"PONG").await?;

        eprintln!("Sent PONG to {addr}");
    }
}
