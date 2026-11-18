// Copyright 2026 Sigma Prime Pty Ltd.
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

use std::{
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};

use libp2p_core::{Multiaddr, multiaddr::Protocol};

use crate::connection::pool::concurrent_dial::PendingDial;

// The 250ms value is from happy eyeballs RFC 8305. This is a rough estimate of 1 RTT
// Duration by which TCP dials are delayed relative to the last QUIC dial
const PUBLIC_TCP_DELAY: Duration = Duration::from_millis(250);
const PRIVATE_TCP_DELAY: Duration = Duration::from_millis(30);

// duration by which QUIC dials are delayed relative to previous QUIC dial
const PUBLIC_QUIC_DELAY: Duration = Duration::from_millis(250);
const PRIVATE_QUIC_DELAY: Duration = Duration::from_millis(30);

// RelayDelay is the duration by which relay dials are delayed relative to direct addresses
const RELAY_DELAY: Duration = Duration::from_millis(250);

// delay for other transport addresses.
const PUBLIC_OTHER_DELAY: Duration = Duration::from_millis(1000);
const PRIVATE_OTHER_DELAY: Duration = Duration::from_millis(100);

/// Rank dial addresses by transport priority and assign staggered delays.
///
/// Returns the dials sorted by dial priority with each paired with its delay.
///
/// Dials are grouped into four categories, dialed in this order:
/// 1. **Private** — addresses on private/link-local IPs or localhost (fastest dials)
/// 2. **Public** — addresses with public IPv4/IPv6
/// 3. **Relay** — addresses containing `/p2p-circuit`
/// 4. **Other** — addresses without any IP component (no `Ip4`/`Ip6`), e.g. DNS-only multiaddrs
///    like `/dns/example.com/tcp/443`
///
/// Within each category, [`group_delays`] sorts by transport priority
/// (QUICv1 < QUIC < WebTransport < TCP < WebRTC < other), IPv6 before IPv4,
/// and lower port first. Happy Eyeballs (RFC 8305) interleaves an IPv4 address
/// early so both address families race each other. Higher-priority addresses
/// get a shorter delay and are dialed first.
pub(crate) fn rank_dials(dials: Vec<PendingDial>) -> Vec<(Duration, PendingDial)> {
    let mut relay = vec![];
    let mut public = vec![];
    let mut private = vec![];
    let mut other = vec![];
    let mut result = Vec::with_capacity(dials.len());

    for dial in dials {
        if dial.addr.iter().any(|p| matches!(p, Protocol::P2pCircuit)) {
            relay.push(dial);
        } else if !is_global_addr(&dial.addr) {
            private.push(dial);
        } else if dial
            .addr
            .iter()
            .any(|p| matches!(p, Protocol::Ip4(_) | Protocol::Ip6(_)))
        {
            public.push(dial);
        } else {
            other.push(dial);
        }
    }

    result.extend(group_delays(
        private,
        PRIVATE_TCP_DELAY,
        PRIVATE_QUIC_DELAY,
        PRIVATE_OTHER_DELAY,
        Duration::ZERO,
    ));
    let relay_offset = if public.is_empty() {
        Duration::ZERO
    } else {
        RELAY_DELAY
    };
    result.extend(group_delays(
        public,
        PUBLIC_TCP_DELAY,
        PUBLIC_QUIC_DELAY,
        PUBLIC_OTHER_DELAY,
        Duration::ZERO,
    ));

    result.extend(group_delays(
        relay,
        PUBLIC_TCP_DELAY,
        PUBLIC_QUIC_DELAY,
        PUBLIC_OTHER_DELAY,
        relay_offset,
    ));

    let max_delay = result.last().map(|d| d.0);
    result.extend(other.into_iter().map(|d| {
        if let Some(max_delay) = max_delay {
            (max_delay + PUBLIC_OTHER_DELAY, d)
        } else {
            (Duration::ZERO, d)
        }
    }));

    result
}

/// Sort addresses by priority and assign staggered dial delays.
///
/// Addresses are sorted so faster transports (QUIC > TCP) and IPv6 are dialed first.
/// Happy Eyeballs (RFC 8305) then interleaves IPv4 addresses early in each transport group.
/// Each address gets a delay so higher-priority addresses start dialing first,
/// reducing wasted attempts on slower paths.
fn group_delays(
    mut dials: Vec<PendingDial>,
    tcp_delay: Duration,
    quic_delay: Duration,
    other_delay: Duration,
    offset: Duration,
) -> Vec<(Duration, PendingDial)> {
    if dials.is_empty() {
        return vec![];
    }

    // Step 1: Sort by transport priority, then IPv6 before IPv4, then lower port first
    dials.sort_by_key(score);

    let is_quic = |a: &Multiaddr| {
        a.iter()
            .any(|p| matches!(p, Protocol::Quic | Protocol::QuicV1))
    };
    let is_tcp = |a: &Multiaddr| a.iter().any(|p| matches!(p, Protocol::Tcp(_)));
    let is_ipv6 = |a: &Multiaddr| a.iter().any(|p| matches!(p, Protocol::Ip6(_)));
    let is_ipv4 = |a: &Multiaddr| a.iter().any(|p| matches!(p, Protocol::Ip4(_)));

    // Phase 2: Single-pass Happy Eyeballs reorder.
    // After sorting, IPv6 addresses precede IPv4 addresses within each transport group.
    // If the first QUIC (or TCP) address is IPv6, we interleave the first IPv4
    // of the same transport at position 1, so both address families race each other
    // per RFC 8305.
    let mut reordered = Vec::with_capacity(dials.len());
    let mut quic_need_ipv4 = false;
    let mut quic_he_applied = false;
    let mut tcp_need_ipv4 = false;
    let mut tcp_he_applied = false;
    let mut first_tcp_idx = None;

    for dial in dials.drain(..) {
        if is_quic(&dial.addr) {
            if !reordered.is_empty() && quic_need_ipv4 && is_ipv4(&dial.addr) {
                reordered.insert(1, dial);
                quic_need_ipv4 = false;
                quic_he_applied = true;
            } else {
                if is_ipv6(&dial.addr) {
                    quic_need_ipv4 = true;
                }
                reordered.push(dial);
            }
        } else if is_tcp(&dial.addr) {
            if let Some(idx) = first_tcp_idx
                && tcp_need_ipv4
                && is_ipv4(&dial.addr)
            {
                reordered.insert(idx + 1, dial);
                tcp_need_ipv4 = false;
                tcp_he_applied = true;
            } else {
                first_tcp_idx.get_or_insert(reordered.len());
                if is_ipv6(&dial.addr) {
                    tcp_need_ipv4 = true;
                }
                reordered.push(dial);
            }
        } else {
            reordered.push(dial);
        }
    }

    // Step 3: Assign delays — how long to wait before starting each dial.
    //
    // QUIC addresses get dialed first. Position 0 starts immediately (0ms),
    // position 1 waits `quic_delay` (default 250ms), and the rest wait
    // `quic_delay` each — or `2 * quic_delay` if Happy Eyeballs reordered
    // an IPv4 address into position 1 (to keep remaining IPv6 probes staggered).
    //
    // TCP addresses start after all QUIC probes, gated by `tcp_start_delay`
    // (= last QUIC delay + `tcp_delay`). Within TCP, the same pattern applies:
    // first at 0 (relative to `tcp_start_delay`), next at `tcp_delay`, rest
    // at `tcp_delay` or `2 * tcp_delay` with Happy Eyeballs.
    //
    // Other transports (WebTransport, WebRTC, etc.) fire after QUIC and TCP,
    // at `base_delay + other_delay`.
    //
    // `base_delay` tracks the most recent QUIC/TCP delay for this purpose.
    let mut result = Vec::with_capacity(reordered.len());
    let mut quic_count = 0;
    let mut tcp_count = 0;
    let mut tcp_start_delay = Duration::ZERO;
    let mut base_delay = Duration::ZERO;
    for dial in reordered {
        let delay = if is_quic(&dial.addr) {
            let d = match quic_count {
                0 => Duration::ZERO,
                1 => quic_delay,
                _ => {
                    if quic_he_applied {
                        2 * quic_delay
                    } else {
                        quic_delay
                    }
                }
            };
            quic_count += 1;
            tcp_start_delay = d + tcp_delay;
            base_delay = d;
            d
        } else if is_tcp(&dial.addr) {
            let d = match tcp_count {
                0 => Duration::ZERO,
                1 => tcp_delay,
                _ => {
                    if tcp_he_applied {
                        2 * tcp_delay
                    } else {
                        tcp_delay
                    }
                }
            };
            tcp_count += 1;
            let d = d + tcp_start_delay;
            base_delay = d;
            d
        } else {
            base_delay + other_delay
        };
        let total = offset + delay;
        result.push((total, dial));
    }

    result
}

/// Score a multiaddress for dialing priority. Lower score = dialed first.
///
/// Ordering: QUICv1 < QUICv0 < WebTransport < TCP < WebRTC < other.
/// Within same transport: IPv6 before IPv4,
/// lower port first as they are more likely to be the peer's listen port.
fn score(dial: &PendingDial) -> (u8, bool, u16) {
    let transport_rank: u8 = if dial.addr.iter().any(|p| matches!(p, Protocol::QuicV1)) {
        0
    } else if dial.addr.iter().any(|p| matches!(p, Protocol::Quic)) {
        1
    } else if dial
        .addr
        .iter()
        .any(|p| matches!(p, Protocol::WebTransport))
    {
        2
    } else if dial.addr.iter().any(|p| matches!(p, Protocol::Tcp(_))) {
        3
    } else if dial
        .addr
        .iter()
        .any(|p| matches!(p, Protocol::WebRTCDirect))
    {
        4
    } else {
        5
    };
    let ipv4_penalty = dial.addr.iter().any(|p| matches!(p, Protocol::Ip4(_)));
    let port = match dial
        .addr
        .iter()
        .find(|p| matches!(p, Protocol::Udp(_) | Protocol::Tcp(_)))
    {
        Some(Protocol::Udp(p)) => p,
        Some(Protocol::Tcp(p)) => p,
        _ => 0u16,
    };
    (transport_rank, ipv4_penalty, port)
}

/// Returns `true` if the address is globally routable.
///
/// Mirrors the nightly `is_global()` on `Ipv4Addr`/`Ipv6Addr`.
/// IPv6 addresses with zone IDs (link-local) and localhost DNS
/// names are not globally routable.
fn is_global_addr(a: &Multiaddr) -> bool {
    if let Some(Protocol::Ip4(ip4)) = a.iter().find(|p| matches!(p, Protocol::Ip4(_))) {
        return is_global_ipv4(&ip4); // no negation
    }
    if let Some(Protocol::Ip6(ip6)) = a.iter().find(|p| matches!(p, Protocol::Ip6(_))) {
        return is_global_ipv6(&ip6); // no negation
    }
    if a.iter().any(|p| matches!(p, Protocol::Ip6zone(_))) {
        return false; // link-local, not globally routable
    }
    if let Some(dns) = a.iter().find_map(|p| match p {
        Protocol::Dns(dns) | Protocol::Dns4(dns) | Protocol::Dns6(dns) => Some(dns),
        _ => None,
    }) {
        return dns == "localhost" || dns.ends_with(".localhost");
    }
    false
}

/// Returns `true` if the address is globally routable.
///
/// Mirrors the unstable `ipv4addr::is_global()` from nightly std.
/// TODO: Remove when `ipv4addr::is_global()` stabilizes
#[allow(clippy::nonminimal_bool)]
fn is_global_ipv4(addr: &Ipv4Addr) -> bool {
    // check if this address is 192.0.0.9 or 192.0.0.10. These addresses are the only two
    // globally routable addresses in the 192.0.0.0/24 range.
    if u32::from_be_bytes(addr.octets()) == 0xc0000009
        || u32::from_be_bytes(addr.octets()) == 0xc000000a
    {
        return true;
    }
    !addr.is_private()
            && !addr.is_loopback()
            && !addr.is_link_local()
            && !addr.is_broadcast()
            && !addr.is_documentation()
            // shared
            && !(addr.octets()[0] == 100 && (addr.octets()[1] & 0b1100_0000 == 0b0100_0000)) &&!(addr.octets()[0] & 240 == 240 && !addr.is_broadcast())
            // addresses reserved for future protocols (`192.0.0.0/24`)
            // reserved
            && !(addr.octets()[0] == 192 && addr.octets()[1] == 0 && addr.octets()[2] == 0)
            // Make sure the address is not in 0.0.0.0/8
            && addr.octets()[0] != 0
}

/// Returns `true` if the address is globally routable.
///
/// Mirrors the unstable `Ipv6Addr::is_global()` from nightly std.
/// TODO: Remove when `ipv4addr::is_global()` stabilizes
const fn is_global_ipv6(addr: &Ipv6Addr) -> bool {
    const fn is_documentation(addr: &Ipv6Addr) -> bool {
        (addr.segments()[0] == 0x2001) && (addr.segments()[1] == 0xdb8)
    }
    const fn is_unique_local(addr: &Ipv6Addr) -> bool {
        (addr.segments()[0] & 0xfe00) == 0xfc00
    }
    const fn is_unicast_link_local(addr: &Ipv6Addr) -> bool {
        (addr.segments()[0] & 0xffc0) == 0xfe80
    }
    !(addr.is_unspecified()
            || addr.is_loopback()
            // IPv4-mapped Address (`::ffff:0:0/96`)
            || matches!(addr.segments(), [0, 0, 0, 0, 0, 0xffff, _, _])
            // IPv4-IPv6 Translat. (`64:ff9b:1::/48`)
            || matches!(addr.segments(), [0x64, 0xff9b, 1, _, _, _, _, _])
            // Discard-Only Address Block (`100::/64`)
            || matches!(addr.segments(), [0x100, 0, 0, 0, _, _, _, _])
            // IETF Protocol Assignments (`2001::/23`)
            || (matches!(addr.segments(), [0x2001, b, _, _, _, _, _, _] if b < 0x200)
                && !(
                    // Port Control Protocol Anycast (`2001:1::1`)
                    u128::from_be_bytes(addr.octets()) == 0x2001_0001_0000_0000_0000_0000_0000_0001
                    // Traversal Using Relays around NAT Anycast (`2001:1::2`)
                    || u128::from_be_bytes(addr.octets()) == 0x2001_0001_0000_0000_0000_0000_0000_0002
                    // AMT (`2001:3::/32`)
                    || matches!(addr.segments(), [0x2001, 3, _, _, _, _, _, _])
                    // AS112-v6 (`2001:4:112::/48`)
                    || matches!(addr.segments(), [0x2001, 4, 0x112, _, _, _, _, _])
                    // ORCHIDv2 (`2001:20::/28`)
                    || matches!(addr.segments(), [0x2001, b, _, _, _, _, _, _] if b >= 0x20 && b <= 0x2F)
                ))
            || is_documentation(addr)
            || is_unique_local(addr)
            || is_unicast_link_local(addr))
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::*;

    fn make_dials(addrs: Vec<Multiaddr>) -> Vec<PendingDial> {
        addrs
            .into_iter()
            .map(|addr| PendingDial {
                addr,
                fut: futures::future::pending().boxed(),
            })
            .collect()
    }

    // Verifies that three QUIC-v1 IPv4 addresses are ranked with the first immediate
    // and the rest delayed by PUBLIC_QUIC_DELAY.
    #[test]
    fn test_quic_delay_ipv4() {
        let q1v1: Multiaddr = "/ip4/1.2.3.4/udp/1/quic-v1".parse().unwrap();
        let q2v1: Multiaddr = "/ip4/1.2.3.4/udp/2/quic-v1".parse().unwrap();
        let q3v1: Multiaddr = "/ip4/1.2.3.4/udp/3/quic-v1".parse().unwrap();

        let dials = make_dials(vec![q1v1.clone(), q2v1.clone(), q3v1.clone()]);
        let output: Vec<_> = rank_dials(dials)
            .into_iter()
            .map(|(d, a)| (a.addr, d))
            .collect();
        assert_eq!(
            output,
            vec![
                (q1v1, Duration::ZERO),
                (q2v1, PUBLIC_QUIC_DELAY),
                (q3v1, PUBLIC_QUIC_DELAY),
            ]
        )
    }

    // Verifies that three QUIC-v1 IPv6 addresses follow the same delay pattern as IPv4.
    #[test]
    fn test_quic_delay_ipv6() {
        let q1v16: Multiaddr = "/ip6/1::2/udp/1/quic-v1".parse().unwrap();
        let q2v16: Multiaddr = "/ip6/1::2/udp/2/quic-v1".parse().unwrap();
        let q3v16: Multiaddr = "/ip6/1::2/udp/3/quic-v1".parse().unwrap();

        let dials = make_dials(vec![q1v16.clone(), q2v16.clone(), q3v16.clone()]);
        let output: Vec<_> = rank_dials(dials)
            .into_iter()
            .map(|(d, a)| (a.addr, d))
            .collect();
        assert_eq!(
            output,
            vec![
                (q1v16, Duration::ZERO),
                (q2v16, PUBLIC_QUIC_DELAY),
                (q3v16, PUBLIC_QUIC_DELAY),
            ]
        )
    }

    // Verifies that Happy Eyeballs interleaves IPv4 at position 1 (250ms),
    // then delays remaining probes at 2x (500ms flat) to let the first two
    // probes race before starting the rest.
    #[test]
    fn test_quic_delay_ipv4_ipv6() {
        let q2v1: Multiaddr = "/ip4/1.2.3.4/udp/2/quic-v1".parse().unwrap();
        let q3v1: Multiaddr = "/ip4/1.2.4.5/udp/2/quic-v1".parse().unwrap();
        let q1v16: Multiaddr = "/ip6/1::2/udp/1/quic-v1".parse().unwrap();
        let q4v16: Multiaddr = "/ip6/1::3/udp/1/quic-v1".parse().unwrap();

        let dials = make_dials(vec![
            q1v16.clone(),
            q2v1.clone(),
            q3v1.clone(),
            q4v16.clone(),
        ]);
        let output: Vec<_> = rank_dials(dials)
            .into_iter()
            .map(|(d, a)| (a.addr, d))
            .collect();
        assert_eq!(
            output,
            vec![
                (q1v16, Duration::ZERO),
                (q2v1, PUBLIC_QUIC_DELAY),
                (q4v16, PUBLIC_QUIC_DELAY * 2),
                (q3v1, PUBLIC_QUIC_DELAY * 2)
            ]
        )
    }

    // Verifies that one QUIC + three TCP addresses sort TCP with IPv6 before IPv4,
    // and TCP starts after all QUIC probes.
    #[test]
    fn test_quic_with_tcp_ipv6_ipv4() {
        let q1v1: Multiaddr = "/ip4/1.2.3.4/udp/1/quic-v1".parse().unwrap();
        let q2v1: Multiaddr = "/ip4/1.2.3.4/udp/2/quic-v1".parse().unwrap();

        let q1v16: Multiaddr = "/ip6/1::2/udp/1/quic-v1".parse().unwrap();
        let q2v16: Multiaddr = "/ip6/1::2/udp/2/quic-v1".parse().unwrap();
        let q3v16: Multiaddr = "/ip6/1::2/udp/3/quic-v1".parse().unwrap();

        let t1: Multiaddr = "/ip4/1.2.3.5/tcp/1".parse().unwrap();
        let t1v6: Multiaddr = "/ip6/1::2/tcp/1".parse().unwrap();
        let t2: Multiaddr = "/ip4/1.2.3.4/tcp/2".parse().unwrap();
        let t3: Multiaddr = "/ip4/1.2.3.4/tcp/3".parse().unwrap();

        let dials = make_dials(vec![
            q1v1.clone(),
            q1v16.clone(),
            q2v16.clone(),
            q3v16.clone(),
            q2v1.clone(),
            t1.clone(),
            t1v6.clone(),
            t2.clone(),
            t3.clone(),
        ]);
        let output: Vec<_> = rank_dials(dials)
            .into_iter()
            .map(|(d, a)| (a.addr, d))
            .collect();
        assert_eq!(
            output,
            vec![
                (q1v16, Duration::ZERO),
                (q1v1, PUBLIC_QUIC_DELAY),
                (q2v16, 2 * PUBLIC_QUIC_DELAY),
                (q3v16, 2 * PUBLIC_QUIC_DELAY),
                (q2v1, 2 * PUBLIC_QUIC_DELAY),
                (t1v6, 3 * PUBLIC_QUIC_DELAY),
                (t1, 4 * PUBLIC_QUIC_DELAY),
                (t2, 5 * PUBLIC_QUIC_DELAY),
                (t3, 5 * PUBLIC_QUIC_DELAY),
            ]
        )
    }

    // Verifies that one QUIC + three TCP addresses sort TCP with IPv6 before IPv4,
    // and TCP starts after all QUIC probes.
    #[test]
    fn test_quic_ip4_with_tcp() {
        let q1v1: Multiaddr = "/ip4/1.2.3.4/udp/1/quic-v1".parse().unwrap();

        let t1: Multiaddr = "/ip4/1.2.3.5/tcp/1".parse().unwrap();
        let t1v6: Multiaddr = "/ip6/1::2/tcp/1".parse().unwrap();
        let t2: Multiaddr = "/ip4/1.2.3.4/tcp/2".parse().unwrap();

        let dials = make_dials(vec![q1v1.clone(), t2.clone(), t1v6.clone(), t1.clone()]);
        let output: Vec<_> = rank_dials(dials)
            .into_iter()
            .map(|(d, a)| (a.addr, d))
            .collect();
        assert_eq!(
            output,
            vec![
                (q1v1, Duration::ZERO),
                (t1v6, PUBLIC_QUIC_DELAY),
                (t1, 2 * PUBLIC_QUIC_DELAY),
                (t2, 3 * PUBLIC_QUIC_DELAY),
            ]
        )
    }

    // Verifies that one QUIC + three all-IPv4 TCP addresses start TCP after QUIC,
    // with the first TCP at PUBLIC_TCP_DELAY.
    #[test]
    fn test_quic_ip4_with_tcp_ipv4() {
        let q1v1: Multiaddr = "/ip4/1.2.3.4/udp/1/quic-v1".parse().unwrap();

        let t1: Multiaddr = "/ip4/1.2.3.5/tcp/1".parse().unwrap();
        let t2: Multiaddr = "/ip4/1.2.3.4/tcp/2".parse().unwrap();
        let t3: Multiaddr = "/ip4/1.2.3.4/tcp/3".parse().unwrap();

        let dials = make_dials(vec![q1v1.clone(), t2.clone(), t3.clone(), t1.clone()]);
        let output: Vec<_> = rank_dials(dials)
            .into_iter()
            .map(|(d, a)| (a.addr, d))
            .collect();
        assert_eq!(
            output,
            vec![
                (q1v1, Duration::ZERO),
                (t1, PUBLIC_TCP_DELAY),
                (t2, 2 * PUBLIC_QUIC_DELAY),
                (t3, 2 * PUBLIC_TCP_DELAY),
            ]
        )
    }

    // Verifies that one QUIC + one TCP IPv6 + one TCP IPv4 rank IPv6 TCP before IPv4 TCP.
    #[test]
    fn test_quic_ip4_with_two_tcp() {
        let q1v1: Multiaddr = "/ip4/1.2.3.4/udp/1/quic-v1".parse().unwrap();

        let t1v6: Multiaddr = "/ip6/1::2/tcp/1".parse().unwrap();
        let t2: Multiaddr = "/ip4/1.2.3.4/tcp/2".parse().unwrap();

        let dials = make_dials(vec![q1v1.clone(), t1v6.clone(), t2.clone()]);
        let output: Vec<_> = rank_dials(dials)
            .into_iter()
            .map(|(d, a)| (a.addr, d))
            .collect();
        assert_eq!(
            output,
            vec![
                (q1v1, Duration::ZERO),
                (t1v6, PUBLIC_TCP_DELAY),
                (t2, 2 * PUBLIC_TCP_DELAY),
            ]
        )
    }

    // Verifies that TCP-only addresses rank IPv6 before IPv4 with Happy Eyeballs
    // interleaving of the first IPv4 address.
    #[test]
    fn test_tcp_ip4_ip6() {
        let t1: Multiaddr = "/ip4/1.2.3.5/tcp/1".parse().unwrap();
        let t1v6: Multiaddr = "/ip6/1::2/tcp/1".parse().unwrap();
        let t2: Multiaddr = "/ip4/1.2.3.4/tcp/2".parse().unwrap();
        let t3: Multiaddr = "/ip4/1.2.3.4/tcp/3".parse().unwrap();

        let dials = make_dials(vec![t1.clone(), t2.clone(), t1v6.clone(), t3.clone()]);
        let output: Vec<_> = rank_dials(dials)
            .into_iter()
            .map(|(d, a)| (a.addr, d))
            .collect();
        assert_eq!(
            output,
            vec![
                (t1v6, Duration::ZERO),
                (t1, PUBLIC_TCP_DELAY),
                (t2, 2 * PUBLIC_TCP_DELAY),
                (t3, 2 * PUBLIC_TCP_DELAY),
            ]
        )
    }

    // Verifies that an empty input produces an empty output.
    #[test]
    fn test_empty() {
        let dials = make_dials(vec![]);
        let output = rank_dials(dials);
        assert!(output.is_empty())
    }
}
