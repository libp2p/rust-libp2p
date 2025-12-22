// Copyright 2021 Protocol Labs.
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

#[cfg(test)]
mod tests;

use std::{borrow::Cow, cmp::Ordering};

use libp2p_core::multiaddr::Protocol;

use super::*;

// The 250ms value is from happy eyeballs RFC 8305. This is a rough estimate of 1 RTT
// Duration by which TCP dials are delayed relative to the last QUIC dial
const PUBLIC_TCP_DELAY: Duration = Duration::from_millis(250);
const PRIVATE_TCP_DELAY: Duration = Duration::from_millis(30);

// duration by which QUIC dials are delayed relative to previous QUIC dial
const PUBLIC_QUIC_DELAY: Duration = Duration::from_millis(250);
const PRIVATE_QUIC_DELAY: Duration = Duration::from_millis(30);

// RelayDelay is the duration by which relay dials are delayed relative to direct addresses
const RELAY_DELAY: Duration = Duration::from_millis(250);

// delay for other transport addresses. This will apply to /webrtc-direct.
const PUBLIC_OTHER_DELAY: Duration = Duration::from_millis(1000);
const PRIVATE_OTHER_DELAY: Duration = Duration::from_millis(100);

pub(crate) type DialRanker = fn(Vec<Multiaddr>) -> Vec<(Multiaddr, Option<Duration>)>;

// Ported from <https://github.com/libp2p/go-libp2p/blob/v0.45.0/p2p/net/swarm/dial_ranker.go#L81>
pub(crate) fn smart_dial_ranker(
    mut addresses: Vec<Multiaddr>,
) -> Vec<(Multiaddr, Option<Duration>)> {
    let mut relay = vec![];
    addresses.retain(|a| {
        if a.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
            relay.push(a.clone());
            false
        } else {
            true
        }
    });
    let mut private = vec![];
    addresses.retain(|a| {
        if let Some(Protocol::Ip4(ip4)) = a.iter().find(|p| matches!(p, Protocol::Ip4(_))) {
            if ip4.is_private() {
                private.push(a.clone());
                false
            } else {
                true
            }
        } else if a.iter().any(|p| matches!(p, Protocol::Ip6zone(_))) {
            private.push(a.clone());
            false
        } else if let Some(dns) = get_dns(a) {
            if is_dns_private(dns.as_ref()) {
                private.push(a.clone());
                false
            } else {
                true
            }
        } else {
            true
        }
    });
    let mut public = vec![];
    addresses.retain(|a| {
        if a.iter()
            .any(|p| matches!(p, Protocol::Ip4(_) | Protocol::Ip6(_)))
        {
            public.push(a.clone());
            false
        } else {
            true
        }
    });
    let relay_offset = if public.is_empty() {
        Duration::ZERO
    } else {
        RELAY_DELAY
    };
    let mut result = Vec::with_capacity(addresses.len());
    result.extend(get_addresses_delay(
        private,
        PRIVATE_TCP_DELAY,
        PRIVATE_QUIC_DELAY,
        PRIVATE_OTHER_DELAY,
        Duration::ZERO,
    ));
    result.extend(get_addresses_delay(
        public,
        PUBLIC_TCP_DELAY,
        PUBLIC_QUIC_DELAY,
        PUBLIC_OTHER_DELAY,
        Duration::ZERO,
    ));
    result.extend(get_addresses_delay(
        relay,
        PUBLIC_TCP_DELAY,
        PUBLIC_QUIC_DELAY,
        PUBLIC_OTHER_DELAY,
        relay_offset,
    ));
    let max_delay = if let Some((_, Some(delay))) = result.last() {
        *delay
    } else {
        Duration::ZERO
    };
    result.extend(
        addresses
            .into_iter()
            .map(|a| (a, Some(max_delay + PUBLIC_OTHER_DELAY))),
    );
    result
}

// Ported from <https://github.com/libp2p/go-libp2p/blob/v0.45.0/p2p/net/swarm/dial_ranker.go#L112>
fn get_addresses_delay(
    mut addresses: Vec<Multiaddr>,
    tcp_delay: Duration,
    quic_delay: Duration,
    other_delay: Duration,
    offset: Duration,
) -> Vec<(Multiaddr, Option<Duration>)> {
    if addresses.is_empty() {
        return vec![];
    }

    addresses.sort_by_key(score);

    // addrs is now sorted by (Transport, IPVersion). Reorder addrs for happy eyeballs dialing.
    // For QUIC and TCP, if we have both IPv6 and IPv4 addresses, move the
    // highest priority IPv4 address to the second position.
    let mut happy_eyeballs_quic = false;
    let mut happy_eyeballs_tcp = false;
    let mut tcp_start_index = 0;
    {
        // If the first QUIC address is IPv6 move the first QUIC IPv4 address to second position
        if is_quic_address(&addresses[0])
            && addresses[0].iter().any(|p| matches!(p, Protocol::Ip6(_)))
        {
            for j in 1..addresses.len() {
                let addr = &addresses[j];
                if is_quic_address(addr) && addr.iter().any(|p| matches!(p, Protocol::Ip4(_))) {
                    // The first IPv4 address is at position j
                    // Move the jth element at position 1 shifting the affected elements
                    if j > 1 {
                        let tmp = addresses.remove(j);
                        addresses.insert(1, tmp);
                    }
                    happy_eyeballs_quic = true;
                    tcp_start_index = j + 1;
                    break;
                }
            }
        }

        while tcp_start_index < addresses.len() {
            if addresses[tcp_start_index]
                .iter()
                .any(|p| matches!(p, Protocol::Tcp(_)))
            {
                break;
            }
            tcp_start_index += 1;
        }

        // If the first TCP address is IPv6 move the first TCP IPv4 address to second position
        if tcp_start_index < addresses.len()
            && addresses[tcp_start_index]
                .iter()
                .any(|p| matches!(p, Protocol::Ip6(_)))
        {
            for j in (tcp_start_index + 1)..addresses.len() {
                let addr = &addresses[j];
                if addr.iter().any(|p| matches!(p, Protocol::Tcp(_)))
                    && addr.iter().any(|p| matches!(p, Protocol::Ip4(_)))
                {
                    // First TCP IPv4 address is at position j, move it to position tcpStartIdx+1
                    // which is the second priority TCP address
                    if j > tcp_start_index + 1 {
                        let tmp = addresses.remove(j);
                        addresses.insert(tcp_start_index + 1, tmp);
                    }
                    happy_eyeballs_tcp = true;
                    break;
                }
            }
        }
    }

    let mut result = Vec::with_capacity(addresses.len());
    let mut tcp_first_dial_delay = Duration::ZERO;
    let mut last_quic_or_tcp_delay = Duration::ZERO;
    for (i, addr) in addresses.into_iter().enumerate() {
        let mut delay = Duration::ZERO;
        if is_quic_address(&addr) {
            // We dial an IPv6 address, then after quicDelay an IPv4
            // address, then after a further quicDelay we dial the rest of the addresses.
            match i.cmp(&1) {
                Ordering::Equal => {
                    delay = quic_delay;
                }
                Ordering::Greater => {
                    // If we have happy eyeballs for QUIC, dials after the second position
                    // will be delayed by 2*quicDelay
                    if happy_eyeballs_quic {
                        delay = 2 * quic_delay;
                    } else {
                        delay = quic_delay;
                    }
                }
                _ => {}
            }
            last_quic_or_tcp_delay = delay;
            tcp_first_dial_delay = delay + tcp_delay;
        } else if addr.iter().any(|p| matches!(p, Protocol::Tcp(_))) {
            // We dial an IPv6 address, then after tcpDelay an IPv4
            // address, then after a further tcpDelay we dial the rest of the addresses.
            match i.cmp(&(tcp_start_index + 1)) {
                Ordering::Equal => {
                    delay = tcp_delay;
                }
                Ordering::Greater => {
                    // If we have happy eyeballs for TCP, dials after the second position
                    // will be delayed by 2*tcpDelay
                    if happy_eyeballs_tcp {
                        delay = 2 * tcp_delay;
                    } else {
                        delay = tcp_delay;
                    }
                }
                _ => {}
            }
            delay += tcp_first_dial_delay;
            last_quic_or_tcp_delay = delay;
        } else {
            // if it's neither quic, webtransport, tcp, or websocket address
            delay = last_quic_or_tcp_delay + other_delay;
        }
        match offset + delay {
            Duration::ZERO => {
                result.push((addr, None));
            }
            d => {
                result.push((addr, Some(d)));
            }
        }
    }
    result
}

// Ported from <https://github.com/libp2p/go-libp2p/blob/v0.45.0/p2p/net/swarm/dial_ranker.go#L226>
fn score(a: &Multiaddr) -> i32 {
    let mut ip4_weight = 0;
    if a.iter().any(|p| matches!(p, Protocol::Ip4(_))) {
        ip4_weight = 1 << 18;
    }

    if a.iter().any(|p| matches!(p, Protocol::WebTransport)) {
        if let Some(Protocol::Udp(p)) = a.iter().find(|p| matches!(p, Protocol::Udp(_))) {
            return ip4_weight + (1 << 19) + p as i32;
        }
    }

    if a.iter().any(|p| matches!(p, Protocol::Quic)) {
        if let Some(Protocol::Udp(p)) = a.iter().find(|p| matches!(p, Protocol::Udp(_))) {
            return ip4_weight + (1 << 17) + p as i32;
        }
    }

    if a.iter().any(|p| matches!(p, Protocol::QuicV1)) {
        if let Some(Protocol::Udp(p)) = a.iter().find(|p| matches!(p, Protocol::Udp(_))) {
            return ip4_weight + p as i32;
        }
    }

    if let Some(Protocol::Tcp(p)) = a.iter().find(|p| matches!(p, Protocol::Tcp(_))) {
        return ip4_weight + (1 << 20) + p as i32;
    }

    if a.iter().any(|p| matches!(p, Protocol::WebRTCDirect)) {
        return 1 << 21;
    }

    1 << 30
}

fn is_quic_address(a: &Multiaddr) -> bool {
    a.iter()
        .any(|p| matches!(p, Protocol::Quic | Protocol::QuicV1))
}

fn get_dns(a: &Multiaddr) -> Option<Cow<'_, str>> {
    if let Some(Protocol::Dns(dns)) = a.iter().find(|p| matches!(p, Protocol::Dns(_))) {
        Some(dns)
    } else if let Some(Protocol::Dns4(dns)) = a.iter().find(|p| matches!(p, Protocol::Dns4(_))) {
        Some(dns)
    } else if let Some(Protocol::Dns6(dns)) = a.iter().find(|p| matches!(p, Protocol::Dns6(_))) {
        Some(dns)
    } else {
        None
    }
}

fn is_dns_private(dns: &str) -> bool {
    dns == "localhost" || dns.ends_with(".localhost")
}
