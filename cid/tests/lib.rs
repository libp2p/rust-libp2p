extern crate cid;
extern crate multihash;

use cid::{Cid, Version, Codec, Error, Prefix};

#[test]
fn basic_marshalling() {
    let h = multihash::encode(multihash::Hash::SHA2256, b"beep boop").unwrap();

    let cid = Cid::new(Codec::DagProtobuf, Version::V1, &h);

    let data = cid.to_bytes();
    let out = Cid::from(data).unwrap();

    assert_eq!(cid, out);

    let s = cid.to_string();
    let out2 = Cid::from(&s[..]).unwrap();

    assert_eq!(cid, out2);
}

#[test]
fn empty_string() {
    assert_eq!(Cid::from(""), Err(Error::InputTooShort));
}

#[test]
fn v0_handling() {
    let old = "QmdfTbBqBPQ7VNxZEYEj14VmRuZBkqFbiwReogJgS1zR1n";
    let cid = Cid::from(old).unwrap();

    assert_eq!(cid.version, Version::V0);
    assert_eq!(cid.to_string(), old);
}

#[test]
fn v0_error() {
    let bad = "QmdfTbBqBPQ7VNxZEYEj14VmRuZBkqFbiwReogJgS1zIII";
    assert_eq!(Cid::from(bad), Err(Error::ParsingError));
}

#[test]
fn prefix_roundtrip() {
    let data = b"awesome test content";
    let h = multihash::encode(multihash::Hash::SHA2256, data).unwrap();

    let cid = Cid::new(Codec::DagProtobuf, Version::V1, &h);
    let prefix = cid.prefix();

    let cid2 = Cid::new_from_prefix(&prefix, data);

    assert_eq!(cid, cid2);

    let prefix_bytes = prefix.as_bytes();
    let prefix2 = Prefix::new_from_bytes(&prefix_bytes).unwrap();

    assert_eq!(prefix, prefix2);
}

#[test]
fn from() {
    let the_hash = "QmdfTbBqBPQ7VNxZEYEj14VmRuZBkqFbiwReogJgS1zR1n";

    let cases = vec![
        format!("/ipfs/{:}", &the_hash),
        format!("https://ipfs.io/ipfs/{:}", &the_hash),
        format!("http://localhost:8080/ipfs/{:}", &the_hash),
    ];

    for case in cases {
        let cid = Cid::from(case).unwrap();
        assert_eq!(cid.version, Version::V0);
        assert_eq!(cid.to_string(), the_hash);
    }
}
