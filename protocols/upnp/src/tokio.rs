// Copyright 2023 Protocol Labs.
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

use std::{error::Error, net::IpAddr};

use futures::{
    channel::{mpsc, oneshot},
    SinkExt, StreamExt,
};
use igd_next::SearchOptions;

pub use crate::behaviour::Behaviour;
use crate::behaviour::{GatewayEvent, GatewayRequest};

// TODO: remove when `IpAddr::is_global` stabilizes.
pub(crate) fn is_addr_global(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => {
            !(ip.octets()[0] == 0 // "This network"
                                    || ip.is_private()
                                    // code for Ipv4::is_shared()
                                    || (ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000 == 0b0100_0000))
                                    || ip.is_loopback()
                                    || ip.is_link_local()
                                    // addresses reserved for future protocols (`192.0.0.0/24`)
                                    ||(ip.octets()[0] == 192 && ip.octets()[1] == 0 && ip.octets()[2] == 0)
                                    || ip.is_documentation()
                                    // code for Ipv4::is_benchmarking()
                                    || (ip.octets()[0] == 198 && (ip.octets()[1] & 0xfe) == 18)
                                    // code for Ipv4::is_reserved()
                                    || (ip.octets()[0] & 240 == 240 && !ip.is_broadcast())
                                    || ip.is_broadcast())
        }
        IpAddr::V6(ip) => {
            !(ip.is_unspecified()
                                    || ip.is_loopback()
                                    // IPv4-mapped Address (`::ffff:0:0/96`)
                                    || matches!(ip.segments(), [0, 0, 0, 0, 0, 0xffff, _, _])
                                    // IPv4-IPv6 Translat. (`64:ff9b:1::/48`)
                                    || matches!(ip.segments(), [0x64, 0xff9b, 1, _, _, _, _, _])
                                    // Discard-Only Address Block (`100::/64`)
                                    || matches!(ip.segments(), [0x100, 0, 0, 0, _, _, _, _])
                                    // IETF Protocol Assignments (`2001::/23`)
                                    || (matches!(ip.segments(), [0x2001, b, _, _, _, _, _, _] if b < 0x200)
                                    && !(
                                    // Port Control Protocol Anycast (`2001:1::1`)
                                    u128::from_be_bytes(ip.octets()) == 0x2001_0001_0000_0000_0000_0000_0000_0001
                                    // Traversal Using Relays around NAT Anycast (`2001:1::2`)
                                    || u128::from_be_bytes(ip.octets()) == 0x2001_0001_0000_0000_0000_0000_0000_0002
                                    // AMT (`2001:3::/32`)
                                    || matches!(ip.segments(), [0x2001, 3, _, _, _, _, _, _])
                                    // AS112-v6 (`2001:4:112::/48`)
                                    || matches!(ip.segments(), [0x2001, 4, 0x112, _, _, _, _, _])
                                    // ORCHIDv2 (`2001:20::/28`)
                                    || matches!(ip.segments(), [0x2001, b, _, _, _, _, _, _] if (0x20..=0x2F).contains(&b))
                                ))
                                    // code for Ipv4::is_documentation()
                                    || (ip.segments()[0] == 0x2001) && (ip.segments()[1] == 0xdb8)
                                    // code for Ipv4::is_unique_local()
                                    || (ip.segments()[0] & 0xfe00) == 0xfc00
                                    // code for Ipv4::is_unicast_link_local()
                                    || (ip.segments()[0] & 0xffc0) == 0xfe80)
        }
    }
}

/// Interface that interacts with the inner gateway by messages,
/// `GatewayRequest`s and `GatewayEvent`s.
#[derive(Debug)]
pub(crate) struct Gateway {
    pub(crate) sender: mpsc::Sender<GatewayRequest>,
    pub(crate) receiver: mpsc::Receiver<GatewayEvent>,
    pub(crate) external_addr: IpAddr,
}

pub(crate) fn search_gateway() -> oneshot::Receiver<Result<Gateway, Box<dyn Error + Send + Sync>>> {
    let (search_result_sender, search_result_receiver) = oneshot::channel();

    let (events_sender, mut task_receiver) = mpsc::channel(10);
    let (mut task_sender, events_queue) = mpsc::channel(0);

    tokio::spawn(async move {
        let gateway = match igd_next::aio::tokio::search_gateway(SearchOptions::default()).await {
            Ok(gateway) => gateway,
            Err(err) => {
                let _ = search_result_sender.send(Err(err.into()));
                return;
            }
        };

        let external_addr = match gateway.get_external_ip().await {
            Ok(addr) => addr,
            Err(err) => {
                let _ = search_result_sender.send(Err(err.into()));
                return;
            }
        };

        // Check if receiver dropped.
        if search_result_sender
            .send(Ok(Gateway {
                sender: events_sender,
                receiver: events_queue,
                external_addr,
            }))
            .is_err()
        {
            return;
        }

        loop {
            // The task sender has dropped so we can return.
            let Some(req) = task_receiver.next().await else {
                return;
            };
            let event = match req {
                GatewayRequest::AddMapping { mapping, duration } => {
                    let gateway = gateway.clone();
                    match gateway
                        .add_port(
                            mapping.protocol,
                            mapping.internal_addr.port(),
                            mapping.internal_addr,
                            duration,
                            "rust-libp2p mapping",
                        )
                        .await
                    {
                        Ok(()) => GatewayEvent::Mapped(mapping),
                        Err(err) => GatewayEvent::MapFailure(mapping, err.into()),
                    }
                }
                GatewayRequest::RemoveMapping(mapping) => {
                    let gateway = gateway.clone();
                    match gateway
                        .remove_port(mapping.protocol, mapping.internal_addr.port())
                        .await
                    {
                        Ok(()) => GatewayEvent::Removed(mapping),
                        Err(err) => GatewayEvent::RemovalFailure(mapping, err.into()),
                    }
                }
            };
            // Gateway was dropped.
            if task_sender.send(event).await.is_err() {
                return;
            }
        }
    });

    search_result_receiver
}
