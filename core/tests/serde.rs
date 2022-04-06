#![cfg(feature = "serde")]

use std::str::FromStr;

use libp2p_core::PeerId;

extern crate _serde as serde;

#[test]
pub fn serialize_peer_id_json() {
    let peer_id = PeerId::from_str("12D3KooWRNw2pJC9748Fmq4WNV27HoSTcX3r37132FLkQMrbKAiC").unwrap();
    let json = serde_json::to_string(&peer_id).unwrap();
    assert_eq!(
        json,
        r#""12D3KooWRNw2pJC9748Fmq4WNV27HoSTcX3r37132FLkQMrbKAiC""#
    )
}

#[test]
pub fn serialize_peer_id_msgpack() {
    let peer_id = PeerId::from_str("12D3KooWRNw2pJC9748Fmq4WNV27HoSTcX3r37132FLkQMrbKAiC").unwrap();
    let buf = rmp_serde::to_vec(&peer_id).unwrap();
    assert_eq!(
        &buf[..],
        &[
            0xc4, 38, // msgpack buffer header
            0x00, 0x24, 0x08, 0x01, 0x12, 0x20, 0xe7, 0x37, 0x0c, 0x66, 0xef, 0xec, 0x80, 0x00,
            0xd5, 0x87, 0xfc, 0x41, 0x65, 0x92, 0x8e, 0xe0, 0x75, 0x5f, 0x94, 0x86, 0xcb, 0x5c,
            0xf0, 0xf7, 0x80, 0xd8, 0xe0, 0x6c, 0x98, 0xce, 0x7d, 0xa9
        ]
    );
}

#[test]
pub fn deserialize_peer_id_json() {
    let peer_id = PeerId::from_str("12D3KooWRNw2pJC9748Fmq4WNV27HoSTcX3r37132FLkQMrbKAiC").unwrap();
    let json = r#""12D3KooWRNw2pJC9748Fmq4WNV27HoSTcX3r37132FLkQMrbKAiC""#;
    assert_eq!(peer_id, serde_json::from_str(json).unwrap())
}

#[test]
pub fn deserialize_peer_id_msgpack() {
    let peer_id = PeerId::from_str("12D3KooWRNw2pJC9748Fmq4WNV27HoSTcX3r37132FLkQMrbKAiC").unwrap();
    let buf = &[
        0xc4, 38, // msgpack buffer header
        0x00, 0x24, 0x08, 0x01, 0x12, 0x20, 0xe7, 0x37, 0x0c, 0x66, 0xef, 0xec, 0x80, 0x00, 0xd5,
        0x87, 0xfc, 0x41, 0x65, 0x92, 0x8e, 0xe0, 0x75, 0x5f, 0x94, 0x86, 0xcb, 0x5c, 0xf0, 0xf7,
        0x80, 0xd8, 0xe0, 0x6c, 0x98, 0xce, 0x7d, 0xa9,
    ];

    assert_eq!(peer_id, rmp_serde::from_read(&mut &buf[..]).unwrap());
}
