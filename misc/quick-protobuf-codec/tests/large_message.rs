use asynchronous_codec::Encoder;
use bytes::BytesMut;
use quick_protobuf_codec::{proto, Codec};

#[test]
fn encode_large_message() {
    let mut codec = Codec::<proto::Message>::new(1_001_000);
    let mut dst = BytesMut::new();
    dst.reserve(1_001_000);
    let message = proto::Message {
        data: vec![0; 1_000_000],
    };

    codec.encode(message, &mut dst).unwrap();
}
