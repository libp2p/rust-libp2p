use data_encoding::HEXUPPER;
use libp2p_identity::PeerId;
use multiaddr::*;
use multihash::Multihash;
use quickcheck::{Arbitrary, Gen, QuickCheck};
use std::{
    borrow::Cow,
    convert::{TryFrom, TryInto},
    iter::{self, FromIterator},
    net::{Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

// Property tests

#[test]
fn to_from_bytes_identity() {
    fn prop(a: Ma) -> bool {
        let b = a.0.to_vec();
        Some(a) == Multiaddr::try_from(b).ok().map(Ma)
    }
    QuickCheck::new().quickcheck(prop as fn(Ma) -> bool)
}

#[test]
fn to_from_str_identity() {
    fn prop(a: Ma) -> bool {
        let b = a.0.to_string();
        Some(a) == Multiaddr::from_str(&b).ok().map(Ma)
    }
    QuickCheck::new().quickcheck(prop as fn(Ma) -> bool)
}

#[test]
fn byteswriter() {
    fn prop(a: Ma, b: Ma) -> bool {
        let mut x = a.0.clone();
        for p in b.0.iter() {
            x = x.with(p)
        }
        x.iter()
            .zip(a.0.iter().chain(b.0.iter()))
            .all(|(x, y)| x == y)
    }
    QuickCheck::new().quickcheck(prop as fn(Ma, Ma) -> bool)
}

#[test]
fn push_pop_identity() {
    fn prop(a: Ma, p: Proto) -> bool {
        let mut b = a.clone();
        let q = p.clone();
        b.0.push(q.0);
        assert_ne!(a.0, b.0);
        Some(p.0) == b.0.pop() && a.0 == b.0
    }
    QuickCheck::new().quickcheck(prop as fn(Ma, Proto) -> bool)
}

#[test]
fn ends_with() {
    fn prop(Ma(m): Ma) {
        let n = m.iter().count();
        for i in 0..n {
            let suffix = m.iter().skip(i).collect::<Multiaddr>();
            assert!(m.ends_with(&suffix));
        }
    }
    QuickCheck::new().quickcheck(prop as fn(_))
}

// Arbitrary impls

#[derive(PartialEq, Eq, Clone, Hash, Debug)]
struct Ma(Multiaddr);

impl Arbitrary for Ma {
    fn arbitrary(g: &mut Gen) -> Self {
        let iter = (0..u8::arbitrary(g) % 128).map(|_| Proto::arbitrary(g).0);
        Ma(Multiaddr::from_iter(iter))
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
struct Proto(Protocol<'static>);

impl Proto {
    const IMPL_VARIANT_COUNT: u8 = 32;
}

impl Arbitrary for Proto {
    fn arbitrary(g: &mut Gen) -> Self {
        use Protocol::*;
        match u8::arbitrary(g) % Proto::IMPL_VARIANT_COUNT {
            0 => Proto(Dccp(Arbitrary::arbitrary(g))),
            1 => Proto(Dns(Cow::Owned(SubString::arbitrary(g).0))),
            2 => Proto(Dns4(Cow::Owned(SubString::arbitrary(g).0))),
            3 => Proto(Dns6(Cow::Owned(SubString::arbitrary(g).0))),
            4 => Proto(Dnsaddr(Cow::Owned(SubString::arbitrary(g).0))),
            5 => Proto(Http),
            6 => Proto(Https),
            7 => Proto(Ip4(Ipv4Addr::arbitrary(g))),
            8 => Proto(Ip6(Ipv6Addr::arbitrary(g))),
            9 => Proto(P2pWebRtcDirect),
            10 => Proto(P2pWebRtcStar),
            11 => Proto(WebRTCDirect),
            12 => Proto(Certhash(Mh::arbitrary(g).0)),
            13 => Proto(P2pWebSocketStar),
            14 => Proto(Memory(Arbitrary::arbitrary(g))),
            15 => {
                let a = iter::repeat_with(|| u8::arbitrary(g))
                    .take(10)
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap();
                Proto(Onion(Cow::Owned(a), std::cmp::max(1, u16::arbitrary(g))))
            }
            16 => {
                let a: [u8; 35] = iter::repeat_with(|| u8::arbitrary(g))
                    .take(35)
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap();
                Proto(Onion3((a, std::cmp::max(1, u16::arbitrary(g))).into()))
            }
            17 => Proto(P2p(PId::arbitrary(g).0)),
            18 => Proto(P2pCircuit),
            19 => Proto(Quic),
            20 => Proto(QuicV1),
            21 => Proto(Sctp(Arbitrary::arbitrary(g))),
            22 => Proto(Tcp(Arbitrary::arbitrary(g))),
            23 => Proto(Tls),
            24 => Proto(Noise),
            25 => Proto(Udp(Arbitrary::arbitrary(g))),
            26 => Proto(Udt),
            27 => Proto(Unix(Cow::Owned(SubString::arbitrary(g).0))),
            28 => Proto(Utp),
            29 => Proto(WebTransport),
            30 => Proto(Ws("/".into())),
            31 => Proto(Wss("/".into())),
            _ => panic!("outside range"),
        }
    }
}

#[derive(Clone, Debug)]
struct Mh(Multihash<64>);

impl Arbitrary for Mh {
    fn arbitrary(g: &mut Gen) -> Self {
        let mut hash: [u8; 32] = [0; 32];
        hash.fill_with(|| u8::arbitrary(g));
        Mh(Multihash::wrap(0x0, &hash).expect("The digest size is never too large"))
    }
}

#[derive(Clone, Debug)]
struct PId(PeerId);

impl Arbitrary for PId {
    fn arbitrary(g: &mut Gen) -> Self {
        let mh = Mh::arbitrary(g);

        PId(PeerId::from_multihash(mh.0).expect("identity multihash works if digest size < 64"))
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
struct SubString(String); // ASCII string without '/'

impl Arbitrary for SubString {
    fn arbitrary(g: &mut Gen) -> Self {
        let mut s = String::arbitrary(g);
        s.retain(|c| c.is_ascii() && c != '/');
        SubString(s)
    }
}

// other unit tests

fn ma_valid(source: &str, target: &str, protocols: Vec<Protocol<'_>>) {
    let parsed = source.parse::<Multiaddr>().unwrap();
    assert_eq!(HEXUPPER.encode(&parsed.to_vec()[..]), target);
    assert_eq!(parsed.iter().collect::<Vec<_>>(), protocols);
    assert_eq!(source.parse::<Multiaddr>().unwrap().to_string(), source);
    assert_eq!(
        Multiaddr::try_from(HEXUPPER.decode(target.as_bytes()).unwrap()).unwrap(),
        parsed
    );
}

fn peer_id(s: &str) -> PeerId {
    s.parse().unwrap()
}

#[test]
fn multiaddr_eq() {
    let m1 = "/ip4/127.0.0.1/udp/1234".parse::<Multiaddr>().unwrap();
    let m2 = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().unwrap();
    let m3 = "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().unwrap();

    assert_ne!(m1, m2);
    assert_ne!(m2, m1);
    assert_eq!(m2, m3);
    assert_eq!(m1, m1);
}

#[test]
fn construct_success() {
    use Protocol::*;

    let local: Ipv4Addr = "127.0.0.1".parse().unwrap();
    let addr6: Ipv6Addr = "2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095".parse().unwrap();

    ma_valid(
        "/ip4/1.2.3.4",
        "0401020304",
        vec![Ip4("1.2.3.4".parse().unwrap())],
    );
    ma_valid(
        "/ip4/0.0.0.0",
        "0400000000",
        vec![Ip4("0.0.0.0".parse().unwrap())],
    );
    ma_valid(
        "/ip6/::1",
        "2900000000000000000000000000000001",
        vec![Ip6("::1".parse().unwrap())],
    );
    ma_valid(
        "/ip6/2601:9:4f81:9700:803e:ca65:66e8:c21",
        "29260100094F819700803ECA6566E80C21",
        vec![Ip6("2601:9:4f81:9700:803e:ca65:66e8:c21".parse().unwrap())],
    );
    ma_valid("/udp/0", "91020000", vec![Udp(0)]);
    ma_valid("/tcp/0", "060000", vec![Tcp(0)]);
    ma_valid("/sctp/0", "84010000", vec![Sctp(0)]);
    ma_valid("/udp/1234", "910204D2", vec![Udp(1234)]);
    ma_valid("/tcp/1234", "0604D2", vec![Tcp(1234)]);
    ma_valid("/sctp/1234", "840104D2", vec![Sctp(1234)]);
    ma_valid("/udp/65535", "9102FFFF", vec![Udp(65535)]);
    ma_valid("/tcp/65535", "06FFFF", vec![Tcp(65535)]);
    ma_valid(
        "/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
        "A503221220D52EBB89D85B02A284948203A62FF28389C57C9F42BEEC4EC20DB76A68911C0B",
        vec![P2p(peer_id(
            "QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
        ))],
    );
    ma_valid(
        "/udp/1234/sctp/1234",
        "910204D2840104D2",
        vec![Udp(1234), Sctp(1234)],
    );
    ma_valid("/udp/1234/udt", "910204D2AD02", vec![Udp(1234), Udt]);
    ma_valid("/udp/1234/utp", "910204D2AE02", vec![Udp(1234), Utp]);
    ma_valid("/tcp/1234/http", "0604D2E003", vec![Tcp(1234), Http]);
    ma_valid(
        "/tcp/1234/tls/http",
        "0604D2C003E003",
        vec![Tcp(1234), Tls, Http],
    );
    ma_valid("/tcp/1234/https", "0604D2BB03", vec![Tcp(1234), Https]);
    ma_valid(
        "/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC/tcp/1234",
        "A503221220D52EBB89D85B02A284948203A62FF28389C57C9F42BEEC4EC20DB76A68911C0B0604D2",
        vec![
            P2p(peer_id("QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC")),
            Tcp(1234),
        ],
    );
    ma_valid(
        "/ip4/127.0.0.1/udp/1234",
        "047F000001910204D2",
        vec![Ip4(local), Udp(1234)],
    );
    ma_valid(
        "/ip4/127.0.0.1/udp/0",
        "047F00000191020000",
        vec![Ip4(local), Udp(0)],
    );
    ma_valid(
        "/ip4/127.0.0.1/tcp/1234",
        "047F0000010604D2",
        vec![Ip4(local), Tcp(1234)],
    );
    ma_valid(
        "/ip4/127.0.0.1/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
        "047F000001A503221220D52EBB89D85B02A284948203A62FF28389C57C9F42BEEC4EC20DB76A68911C0B",
        vec![
            Ip4(local),
            P2p(peer_id("QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC")),
        ],
    );
    ma_valid("/ip4/127.0.0.1/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC/tcp/1234",
             "047F000001A503221220D52EBB89D85B02A284948203A62FF28389C57C9F42BEEC4EC20DB76A68911C0B0604D2",
             vec![Ip4(local), P2p(peer_id("QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC")), Tcp(1234)]);
    // /unix/a/b/c/d/e,
    // /unix/stdio,
    // /ip4/1.2.3.4/tcp/80/unix/a/b/c/d/e/f,
    // /ip4/127.0.0.1/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC/tcp/1234/unix/stdio
    ma_valid("/ip6/2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095/tcp/8000/ws/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
             "29200108A07AC542013AC986FFFE317095061F40DD03A503221220D52EBB89D85B02A284948203A62FF28389C57C9F42BEEC4EC20DB76A68911C0B",
             vec![Ip6(addr6), Tcp(8000), Ws("/".into()), P2p(peer_id("QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC"))
             ]);
    ma_valid("/p2p-webrtc-star/ip4/127.0.0.1/tcp/9090/ws/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
             "9302047F000001062382DD03A503221220D52EBB89D85B02A284948203A62FF28389C57C9F42BEEC4EC20DB76A68911C0B",
             vec![P2pWebRtcStar, Ip4(local), Tcp(9090), Ws("/".into()), P2p(peer_id("QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC"))
             ]);
    ma_valid("/ip6/2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095/tcp/8000/wss/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
             "29200108A07AC542013AC986FFFE317095061F40DE03A503221220D52EBB89D85B02A284948203A62FF28389C57C9F42BEEC4EC20DB76A68911C0B",
             vec![Ip6(addr6), Tcp(8000), Wss("/".into()), P2p(peer_id("QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC"))]);
    ma_valid("/ip4/127.0.0.1/tcp/9090/p2p-circuit/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
             "047F000001062382A202A503221220D52EBB89D85B02A284948203A62FF28389C57C9F42BEEC4EC20DB76A68911C0B",
             vec![Ip4(local), Tcp(9090), P2pCircuit, P2p(peer_id("QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC"))]);

    ma_valid(
        "/onion/aaimaq4ygg2iegci:80",
        "BC030010C0439831B48218480050",
        vec![Onion(
            Cow::Owned([0, 16, 192, 67, 152, 49, 180, 130, 24, 72]),
            80,
        )],
    );
    ma_valid(
        "/onion3/vww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyyd:1234",
        "BD03ADADEC040BE047F9658668B11A504F3155001F231A37F54C4476C07FB4CC139ED7E30304D2",
        vec![Onion3(
            (
                [
                    173, 173, 236, 4, 11, 224, 71, 249, 101, 134, 104, 177, 26, 80, 79, 49, 85, 0,
                    31, 35, 26, 55, 245, 76, 68, 118, 192, 127, 180, 204, 19, 158, 215, 227, 3,
                ],
                1234,
            )
                .into(),
        )],
    );
    ma_valid(
        "/dnsaddr/sjc-1.bootstrap.libp2p.io",
        "3819736A632D312E626F6F7473747261702E6C69627032702E696F",
        vec![Dnsaddr(Cow::Borrowed("sjc-1.bootstrap.libp2p.io"))],
    );
    ma_valid(
        "/dnsaddr/sjc-1.bootstrap.libp2p.io/tcp/1234/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
        "3819736A632D312E626F6F7473747261702E6C69627032702E696F0604D2A50322122006B3608AA000274049EB28AD8E793A26FF6FAB281A7D3BD77CD18EB745DFAABB",
        vec![Dnsaddr(Cow::Borrowed("sjc-1.bootstrap.libp2p.io")), Tcp(1234), P2p(peer_id("QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN"))]
    );
    ma_valid(
        "/ip4/127.0.0.1/tcp/127/ws",
        "047F00000106007FDD03",
        vec![Ip4(local), Tcp(127), Ws("/".into())],
    );
    ma_valid(
        "/ip4/127.0.0.1/tcp/127/tls",
        "047F00000106007FC003",
        vec![Ip4(local), Tcp(127), Tls],
    );
    ma_valid(
        "/ip4/127.0.0.1/tcp/127/tls/ws",
        "047F00000106007FC003DD03",
        vec![Ip4(local), Tcp(127), Tls, Ws("/".into())],
    );

    ma_valid(
        "/ip4/127.0.0.1/tcp/127/noise",
        "047F00000106007FC603",
        vec![Ip4(local), Tcp(127), Noise],
    );

    ma_valid(
        "/ip4/127.0.0.1/udp/1234/webrtc-direct",
        "047F000001910204D29802",
        vec![Ip4(local), Udp(1234), WebRTCDirect],
    );

    let (_base, decoded) =
        multibase::decode("uEiDDq4_xNyDorZBH3TlGazyJdOWSwvo4PUo5YHFMrvDE8g").unwrap();
    ma_valid(
        "/ip4/127.0.0.1/udp/1234/webrtc-direct/certhash/uEiDDq4_xNyDorZBH3TlGazyJdOWSwvo4PUo5YHFMrvDE8g",
        "047F000001910204D29802D203221220C3AB8FF13720E8AD9047DD39466B3C8974E592C2FA383D4A3960714CAEF0C4F2",
        vec![
            Ip4(local),
            Udp(1234),
            WebRTCDirect,
            Certhash(Multihash::from_bytes(&decoded).unwrap()),
        ],
    );

    ma_valid(
        "/ip4/127.0.0.1/udp/1234/quic/webtransport",
        "047F000001910204D2CC03D103",
        vec![Ip4(local), Udp(1234), Quic, WebTransport],
    );

    let (_base, decoded) =
        multibase::decode("uEiDDq4_xNyDorZBH3TlGazyJdOWSwvo4PUo5YHFMrvDE8g").unwrap();
    ma_valid(
        "/ip4/127.0.0.1/udp/1234/webtransport/certhash/uEiDDq4_xNyDorZBH3TlGazyJdOWSwvo4PUo5YHFMrvDE8g",
        "047F000001910204D2D103D203221220C3AB8FF13720E8AD9047DD39466B3C8974E592C2FA383D4A3960714CAEF0C4F2",
        vec![
            Ip4(local),
            Udp(1234),
            WebTransport,
            Certhash(Multihash::from_bytes(&decoded).unwrap()),
        ],
    );
}

#[test]
fn construct_fail() {
    let addresses = [
        "/ip4",
        "/ip4/::1",
        "/ip4/fdpsofodsajfdoisa",
        "/ip6",
        "/udp",
        "/tcp",
        "/sctp",
        "/udp/65536",
        "/tcp/65536",
        "/onion/9imaq4ygg2iegci7:80",
        "/onion/aaimaq4ygg2iegci7:80",
        "/onion/timaq4ygg2iegci7:0",
        "/onion/timaq4ygg2iegci7:-1",
        "/onion/timaq4ygg2iegci7",
        "/onion/timaq4ygg2iegci@:666",
        "/onion3/9ww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyyd:80",
        "/onion3/vww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyyd7:80",
        "/onion3/vww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyyd:0",
        "/onion3/vww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyyd:-1",
        "/onion3/vww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyyd",
        "/onion3/vww6ybal4bd7szmgncyruucpgfkqahzddi37ktceo3ah7ngmcopnpyy@:666",
        "/udp/1234/sctp",
        "/udp/1234/udt/1234",
        "/udp/1234/utp/1234",
        "/ip4/127.0.0.1/udp/jfodsajfidosajfoidsa",
        "/ip4/127.0.0.1/udp",
        "/ip4/127.0.0.1/tcp/jfodsajfidosajfoidsa",
        "/ip4/127.0.0.1/tcp",
        "/ip4/127.0.0.1/p2p",
        "/ip4/127.0.0.1/p2p/tcp",
        "/p2p-circuit/50",
        "/ip4/127.0.0.1/udp/1234/webrtc-direct/certhash",
        "/ip4/127.0.0.1/udp/1234/webrtc-direct/certhash/b2uaraocy6yrdblb4sfptaddgimjmmp", // 1 character missing from certhash
    ];

    for address in &addresses {
        assert!(
            address.parse::<Multiaddr>().is_err(),
            "{}",
            address.to_string()
        );
    }
}

#[test]
fn to_multiaddr() {
    assert_eq!(
        Multiaddr::from(Ipv4Addr::new(127, 0, 0, 1)),
        "/ip4/127.0.0.1".parse().unwrap()
    );
    assert_eq!(
        Multiaddr::from(Ipv6Addr::new(
            0x2601, 0x9, 0x4f81, 0x9700, 0x803e, 0xca65, 0x66e8, 0xc21
        )),
        "/ip6/2601:9:4f81:9700:803e:ca65:66e8:c21".parse().unwrap()
    );
    assert_eq!(
        Multiaddr::try_from("/ip4/127.0.0.1/tcp/1234".to_string()).unwrap(),
        "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().unwrap()
    );
    assert_eq!(
        Multiaddr::try_from("/ip6/2601:9:4f81:9700:803e:ca65:66e8:c21").unwrap(),
        "/ip6/2601:9:4f81:9700:803e:ca65:66e8:c21"
            .parse::<Multiaddr>()
            .unwrap()
    );
    assert_eq!(
        Multiaddr::from(Ipv4Addr::new(127, 0, 0, 1)).with(Protocol::Tcp(1234)),
        "/ip4/127.0.0.1/tcp/1234".parse::<Multiaddr>().unwrap()
    );
    assert_eq!(
        Multiaddr::from(Ipv6Addr::new(
            0x2601, 0x9, 0x4f81, 0x9700, 0x803e, 0xca65, 0x66e8, 0xc21
        ))
        .with(Protocol::Tcp(1234)),
        "/ip6/2601:9:4f81:9700:803e:ca65:66e8:c21/tcp/1234"
            .parse::<Multiaddr>()
            .unwrap()
    );
}

#[test]
fn from_bytes_fail() {
    let bytes = vec![1, 2, 3, 4];
    assert!(Multiaddr::try_from(bytes).is_err());
}

#[test]
fn ser_and_deser_json() {
    let addr: Multiaddr = "/ip4/0.0.0.0/tcp/0/tls".parse::<Multiaddr>().unwrap();
    let serialized = serde_json::to_string(&addr).unwrap();
    assert_eq!(serialized, "\"/ip4/0.0.0.0/tcp/0/tls\"");
    let deserialized: Multiaddr = serde_json::from_str(&serialized).unwrap();
    assert_eq!(addr, deserialized);
}

#[test]
fn ser_and_deser_bincode() {
    let addr: Multiaddr = "/ip4/0.0.0.0/tcp/0/tls".parse::<Multiaddr>().unwrap();
    let serialized = bincode::serialize(&addr).unwrap();
    // compact addressing
    assert_eq!(
        serialized,
        vec![10, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 6, 0, 0, 192, 3]
    );
    let deserialized: Multiaddr = bincode::deserialize(&serialized).unwrap();
    assert_eq!(addr, deserialized);
}

#[test]
fn append() {
    let mut a: Multiaddr = Protocol::Ip4(Ipv4Addr::new(1, 2, 3, 4)).into();
    a.push(Protocol::Tcp(80));
    a.push(Protocol::Http);

    let mut i = a.iter();
    assert_eq!(Some(Protocol::Ip4(Ipv4Addr::new(1, 2, 3, 4))), i.next());
    assert_eq!(Some(Protocol::Tcp(80)), i.next());
    assert_eq!(Some(Protocol::Http), i.next());
    assert_eq!(None, i.next())
}

fn replace_ip_addr(a: &Multiaddr, p: Protocol<'_>) -> Option<Multiaddr> {
    a.replace(0, move |x| match x {
        Protocol::Ip4(_) | Protocol::Ip6(_) => Some(p),
        _ => None,
    })
}

#[test]
fn replace_ip4_with_ip4() {
    let server = multiaddr!(Ip4(Ipv4Addr::LOCALHOST), Tcp(10000u16));
    let result = replace_ip_addr(&server, Protocol::Ip4([80, 81, 82, 83].into())).unwrap();
    assert_eq!(result, multiaddr!(Ip4([80, 81, 82, 83]), Tcp(10000u16)))
}

#[test]
fn replace_ip6_with_ip4() {
    let server = multiaddr!(Ip6(Ipv6Addr::LOCALHOST), Tcp(10000u16));
    let result = replace_ip_addr(&server, Protocol::Ip4([80, 81, 82, 83].into())).unwrap();
    assert_eq!(result, multiaddr!(Ip4([80, 81, 82, 83]), Tcp(10000u16)))
}

#[test]
fn replace_ip4_with_ip6() {
    let server = multiaddr!(Ip4(Ipv4Addr::LOCALHOST), Tcp(10000u16));
    let result = replace_ip_addr(&server, "2001:db8::1".parse::<Ipv6Addr>().unwrap().into());
    assert_eq!(
        result.unwrap(),
        "/ip6/2001:db8::1/tcp/10000".parse::<Multiaddr>().unwrap()
    )
}

#[test]
fn unknown_protocol_string() {
    match "/unknown/1.2.3.4".parse::<Multiaddr>() {
        Ok(_) => panic!("The UnknownProtocolString error should be caused"),
        Err(e) => match e {
            crate::Error::UnknownProtocolString(protocol) => {
                assert_eq!(protocol, "unknown")
            }
            _ => panic!("The UnknownProtocolString error should be caused"),
        },
    }
}

#[test]
fn protocol_stack() {
    let addresses = [
        "/ip4/0.0.0.0",
        "/ip6/::1",
        "/ip6/2601:9:4f81:9700:803e:ca65:66e8:c21",
        "/udp/0",
        "/tcp/0",
        "/sctp/0",
        "/udp/1234",
        "/tcp/1234",
        "/sctp/1234",
        "/udp/65535",
        "/tcp/65535",
        "/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
        "/udp/1234/sctp/1234",
        "/udp/1234/udt",
        "/udp/1234/utp",
        "/tcp/1234/http",
        "/tcp/1234/tls/http",
        "/tcp/1234/https",
        "/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC/tcp/1234",
        "/ip4/127.0.0.1/udp/1234",
        "/ip4/127.0.0.1/udp/0",
        "/ip4/127.0.0.1/tcp/1234",
        "/ip4/127.0.0.1/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
        "/ip4/127.0.0.1/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC/tcp/1234",
        "/ip6/2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095/tcp/8000/ws/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
        "/p2p-webrtc-star/ip4/127.0.0.1/tcp/9090/ws/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
        "/ip6/2001:8a0:7ac5:4201:3ac9:86ff:fe31:7095/tcp/8000/wss/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
        "/ip4/127.0.0.1/tcp/9090/p2p-circuit/p2p/QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
        "/onion/aaimaq4ygg2iegci:80",
        "/dnsaddr/sjc-1.bootstrap.libp2p.io",
        "/dnsaddr/sjc-1.bootstrap.libp2p.io/tcp/1234/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
        "/ip4/127.0.0.1/tcp/127/ws",
        "/ip4/127.0.0.1/tcp/127/tls",
        "/ip4/127.0.0.1/tcp/127/tls/ws",
        "/ip4/127.0.0.1/tcp/127/noise",
        "/ip4/127.0.0.1/udp/1234/webrtc-direct",
    ];
    let argless = std::collections::HashSet::from([
        "http",
        "https",
        "noise",
        "p2p-circuit",
        "p2p-webrtc-direct",
        "p2p-webrtc-star",
        "p2p-websocket-star",
        "quic",
        "quic-v1",
        "tls",
        "udt",
        "utp",
        "webrtc-direct",
        "ws",
        "wss",
    ]);
    for addr_str in addresses {
        let ma = Multiaddr::from_str(addr_str).expect("These are supposed to be valid multiaddrs");
        let ps: Vec<&str> = ma.protocol_stack().collect();
        let mut toks: Vec<&str> = addr_str.split('/').collect();
        assert_eq!("", toks[0]);
        toks.remove(0);
        let mut i = 0;
        while i < toks.len() {
            let proto_tag = toks[i];
            i += 1;
            if argless.contains(proto_tag) {
                //skip
            } else {
                toks.remove(i);
            }
        }
        assert_eq!(ps, toks);
    }
}

// Assert all `Protocol` variants are covered
// in its `Arbitrary` impl.
#[cfg(nightly)]
#[test]
fn arbitrary_impl_for_all_proto_variants() {
    let variants = core::mem::variant_count::<Protocol>() as u8;
    assert_eq!(variants, Proto::IMPL_VARIANT_COUNT);
}

mod multiaddr_with_p2p {
    use libp2p_identity::PeerId;
    use multiaddr::Multiaddr;

    fn test_multiaddr_with_p2p(
        multiaddr: &str,
        peer: &str,
        expected: std::result::Result<&str, &str>,
    ) {
        let peer = peer.parse::<PeerId>().unwrap();
        let expected = expected
            .map(|a| a.parse::<Multiaddr>().unwrap())
            .map_err(|a| a.parse::<Multiaddr>().unwrap());

        let mut multiaddr = multiaddr.parse::<Multiaddr>().unwrap();
        // Testing multiple time to validate idempotence.
        for _ in 0..3 {
            let result = multiaddr.with_p2p(peer);
            assert_eq!(result, expected);
            multiaddr = result.unwrap_or_else(|addr| addr);
        }
    }

    #[test]
    fn empty_multiaddr() {
        // Multiaddr is empty -> it should push and return Ok.
        test_multiaddr_with_p2p(
            "",
            "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
            Ok("/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN"),
        )
    }
    #[test]
    fn non_p2p_terminated() {
        // Last protocol is not p2p -> it should push and return Ok.
        test_multiaddr_with_p2p(
            "/ip4/127.0.0.1",
            "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
            Ok("/ip4/127.0.0.1/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN"),
        )
    }

    #[test]
    fn p2p_terminated_same_peer() {
        // Last protocol is p2p and the contained peer matches the provided one -> it should do nothing and return Ok.
        test_multiaddr_with_p2p(
            "/ip4/127.0.0.1/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
            "QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
            Ok("/ip4/127.0.0.1/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN"),
        )
    }

    #[test]
    fn p2p_terminated_different_peer() {
        // Last protocol is p2p but the contained peer does not match the provided one -> it should do nothing and return Err.
        test_multiaddr_with_p2p(
            "/ip4/127.0.0.1/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
            "QmcgpsyWgH8Y8ajJz1Cu72KnS5uo2Aa2LpzU7kinSupNKC",
            Err("/ip4/127.0.0.1/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN"),
        )
    }
}
