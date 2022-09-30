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

use anyhow::{Context, Result};
use futures::prelude::*;
use libp2p_core::Transport;
use libp2p_tcp::tokio::TcpStream;
use libp2p_tcp::GenTcpConfig;
use std::time::Duration;
use tokio_crate as tokio;

#[tokio::main]
async fn main() -> Result<()> {
    let mut transport = libp2p_tcp::TokioTcpTransport::new(GenTcpConfig::new()).boxed();

    let addr = std::env::args()
        .skip(1)
        .next()
        .context("Missing argument")?
        .parse()
        .context("Failed to parse argument")?;

    let stream = transport.dial(addr)?.await?;

    ping_pong(stream).await?;

    Ok(())
}

async fn ping_pong(mut stream: TcpStream) -> Result<()> {
    let addr = stream.0.peer_addr()?;

    loop {
        stream.write_all(b"PING").await?;

        eprintln!("Sent PING to {addr}",);

        let mut buffer = [0u8; 4];
        stream.read_exact(&mut buffer).await?;

        anyhow::ensure!(&buffer == b"PONG");

        eprintln!("Received PING from {addr}");

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
