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

//! Tests are ported from <https://github.com/libp2p/go-libp2p/blob/v0.45.0/p2p/net/swarm/dial_ranker_test.go>

use super::*;

#[test]
fn test_quic_delay_ipv4() {
    let q1v1: Multiaddr = "/ip4/1.2.3.4/udp/1/quic-v1".parse().unwrap();
    let q2v1: Multiaddr = "/ip4/1.2.3.4/udp/2/quic-v1".parse().unwrap();
    let q3v1: Multiaddr = "/ip4/1.2.3.4/udp/3/quic-v1".parse().unwrap();

    let addresses = vec![q1v1.clone(), q2v1.clone(), q3v1.clone()];
    let output = smart_dial_ranker(addresses);
    assert_eq!(
        output,
        vec![
            (q1v1, None),
            (q2v1, Some(PUBLIC_QUIC_DELAY)),
            (q3v1, Some(PUBLIC_QUIC_DELAY)),
        ]
    )
}

#[test]
fn test_quic_delay_ipv6() {
    let q1v16: Multiaddr = "/ip6/1::2/udp/1/quic-v1".parse().unwrap();
    let q2v16: Multiaddr = "/ip6/1::2/udp/2/quic-v1".parse().unwrap();
    let q3v16: Multiaddr = "/ip6/1::2/udp/3/quic-v1".parse().unwrap();

    let addresses = vec![q1v16.clone(), q2v16.clone(), q3v16.clone()];
    let output = smart_dial_ranker(addresses);
    assert_eq!(
        output,
        vec![
            (q1v16, None),
            (q2v16, Some(PUBLIC_QUIC_DELAY)),
            (q3v16, Some(PUBLIC_QUIC_DELAY)),
        ]
    )
}

#[test]
fn test_quic_delay_ipv4_ipv6() {
    let q2v1: Multiaddr = "/ip4/1.2.3.4/udp/2/quic-v1".parse().unwrap();
    let q1v16: Multiaddr = "/ip6/1::2/udp/1/quic-v1".parse().unwrap();

    let addresses = vec![q1v16.clone(), q2v1.clone()];
    let output = smart_dial_ranker(addresses);
    assert_eq!(
        output,
        vec![(q1v16, None), (q2v1, Some(PUBLIC_QUIC_DELAY)),]
    )
}

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

    let addresses = vec![
        q1v1.clone(),
        q1v16.clone(),
        q2v16.clone(),
        q3v16.clone(),
        q2v1.clone(),
        t1.clone(),
        t1v6.clone(),
        t2.clone(),
        t3.clone(),
    ];
    let output = smart_dial_ranker(addresses);
    assert_eq!(
        output,
        vec![
            (q1v16, None),
            (q1v1, Some(PUBLIC_QUIC_DELAY)),
            (q2v16, Some(2 * PUBLIC_QUIC_DELAY)),
            (q3v16, Some(2 * PUBLIC_QUIC_DELAY)),
            (q2v1, Some(2 * PUBLIC_QUIC_DELAY)),
            (t1v6, Some(3 * PUBLIC_QUIC_DELAY)),
            (t1, Some(4 * PUBLIC_QUIC_DELAY)),
            (t2, Some(5 * PUBLIC_QUIC_DELAY)),
            (t3, Some(5 * PUBLIC_QUIC_DELAY)),
        ]
    )
}

#[test]
fn test_quic_ip4_with_tcp() {
    let q1v1: Multiaddr = "/ip4/1.2.3.4/udp/1/quic-v1".parse().unwrap();

    let t1: Multiaddr = "/ip4/1.2.3.5/tcp/1".parse().unwrap();
    let t1v6: Multiaddr = "/ip6/1::2/tcp/1".parse().unwrap();
    let t2: Multiaddr = "/ip4/1.2.3.4/tcp/2".parse().unwrap();

    let addresses = vec![q1v1.clone(), t2.clone(), t1v6.clone(), t1.clone()];
    let output = smart_dial_ranker(addresses);
    assert_eq!(
        output,
        vec![
            (q1v1, None),
            (t1v6, Some(PUBLIC_QUIC_DELAY)),
            (t1, Some(2 * PUBLIC_QUIC_DELAY)),
            (t2, Some(3 * PUBLIC_QUIC_DELAY)),
        ]
    )
}

#[test]
fn test_quic_ip4_with_tcp_ipv4() {
    let q1v1: Multiaddr = "/ip4/1.2.3.4/udp/1/quic-v1".parse().unwrap();

    let t1: Multiaddr = "/ip4/1.2.3.5/tcp/1".parse().unwrap();
    let t2: Multiaddr = "/ip4/1.2.3.4/tcp/2".parse().unwrap();
    let t3: Multiaddr = "/ip4/1.2.3.4/tcp/3".parse().unwrap();

    let addresses = vec![q1v1.clone(), t2.clone(), t3.clone(), t1.clone()];
    let output = smart_dial_ranker(addresses);
    assert_eq!(
        output,
        vec![
            (q1v1, None),
            (t1, Some(PUBLIC_TCP_DELAY)),
            (t2, Some(2 * PUBLIC_QUIC_DELAY)),
            (t3, Some(2 * PUBLIC_TCP_DELAY)),
        ]
    )
}

#[test]
fn test_quic_ip4_with_two_tcp() {
    let q1v1: Multiaddr = "/ip4/1.2.3.4/udp/1/quic-v1".parse().unwrap();

    let t1v6: Multiaddr = "/ip6/1::2/tcp/1".parse().unwrap();
    let t2: Multiaddr = "/ip4/1.2.3.4/tcp/2".parse().unwrap();

    let addresses = vec![q1v1.clone(), t1v6.clone(), t2.clone()];
    let output = smart_dial_ranker(addresses);
    assert_eq!(
        output,
        vec![
            (q1v1, None),
            (t1v6, Some(PUBLIC_TCP_DELAY)),
            (t2, Some(2 * PUBLIC_TCP_DELAY)),
        ]
    )
}

#[test]
fn test_tcp_ip4_ip6() {
    let t1: Multiaddr = "/ip4/1.2.3.5/tcp/1".parse().unwrap();
    let t1v6: Multiaddr = "/ip6/1::2/tcp/1".parse().unwrap();
    let t2: Multiaddr = "/ip4/1.2.3.4/tcp/2".parse().unwrap();
    let t3: Multiaddr = "/ip4/1.2.3.4/tcp/3".parse().unwrap();

    let addresses = vec![t1.clone(), t2.clone(), t1v6.clone(), t3.clone()];
    let output = smart_dial_ranker(addresses);
    assert_eq!(
        output,
        vec![
            (t1v6, None),
            (t1, Some(PUBLIC_TCP_DELAY)),
            (t2, Some(2 * PUBLIC_TCP_DELAY)),
            (t3, Some(2 * PUBLIC_TCP_DELAY)),
        ]
    )
}

#[test]
fn test_empty() {
    let addresses = vec![];
    let output = smart_dial_ranker(addresses);
    assert_eq!(output, vec![])
}
