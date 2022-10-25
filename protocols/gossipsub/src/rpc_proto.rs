// Copyright 2020 Sigma Prime Pty Ltd.
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
#![allow(clippy::derive_partial_eq_without_eq)]
mod protos {
    include!(concat!(env!("OUT_DIR"), "/protos/mod.rs"));
}

pub use protos::rpc::*;

#[cfg(test)]
mod test {
    use crate::IdentTopic as Topic;
    use libp2p_core::PeerId;
    use protobuf::Message;
    use rand::Rng;

    mod protos {
        include!(concat!(env!("OUT_DIR"), "/protos/mod.rs"));
    }
    use protos::compat as compat_proto;

    #[test]
    fn test_multi_topic_message_compatibility() {
        let topic1 = Topic::new("t1").hash();
        let topic2 = Topic::new("t2").hash();

        let new_message1 = super::Message {
            from: Some(PeerId::random().to_bytes()),
            data: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            seqno: Some(rand::thread_rng().gen::<[u8; 8]>().to_vec()),
            topic: Some(topic1.clone().into_string()),
            signature: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            key: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            ..super::Message::default()
        };
        let old_message1 = compat_proto::Message {
            from: Some(PeerId::random().to_bytes()),
            data: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            seqno: Some(rand::thread_rng().gen::<[u8; 8]>().to_vec()),
            topic_ids: vec![topic1.clone().into_string()],
            signature: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            key: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            ..compat_proto::Message::default()
        };
        let old_message2 = compat_proto::Message {
            from: Some(PeerId::random().to_bytes()),
            data: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            seqno: Some(rand::thread_rng().gen::<[u8; 8]>().to_vec()),
            topic_ids: vec![topic1.clone().into_string(), topic2.clone().into_string()],
            signature: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            key: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            ..compat_proto::Message::default()
        };

        let new_message1b = new_message1.write_to_bytes().unwrap();

        let old_message1b = old_message1.write_to_bytes().unwrap();

        let old_message2b = old_message2.write_to_bytes().unwrap();

        let new_message = super::Message::parse_from_bytes(&old_message1b[..]).unwrap();
        assert_eq!(new_message.topic.unwrap(), topic1.clone().into_string());

        let new_message = super::Message::parse_from_bytes(&old_message2b[..]).unwrap();
        assert_eq!(new_message.topic.unwrap(), topic2.into_string());

        let old_message = compat_proto::Message::parse_from_bytes(&new_message1b[..]).unwrap();
        assert_eq!(old_message.topic_ids, vec![topic1.into_string()]);
    }
}
