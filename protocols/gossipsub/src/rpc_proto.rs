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

include!(concat!(env!("OUT_DIR"), "/gossipsub.pb.rs"));

#[cfg(test)]
mod test {
    use crate::IdentTopic as Topic;
    use libp2p_core::PeerId;
    use prost::Message;
    use rand::Rng;

    mod compat_proto {
        include!(concat!(env!("OUT_DIR"), "/compat.pb.rs"));
    }

    #[test]
    fn test_multi_topic_message_compatibility() {
        let topic1 = Topic::new("t1").hash();
        let topic2 = Topic::new("t2").hash();

        let new_message1 = super::Message {
            from: Some(PeerId::random().to_bytes()),
            data: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            seqno: Some(rand::thread_rng().gen::<[u8; 8]>().to_vec()),
            topic: topic1.clone().into_string(),
            signature: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            key: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
        };
        let old_message1 = compat_proto::Message {
            from: Some(PeerId::random().to_bytes()),
            data: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            seqno: Some(rand::thread_rng().gen::<[u8; 8]>().to_vec()),
            topic_ids: vec![topic1.clone().into_string()],
            signature: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            key: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
        };
        let old_message2 = compat_proto::Message {
            from: Some(PeerId::random().to_bytes()),
            data: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            seqno: Some(rand::thread_rng().gen::<[u8; 8]>().to_vec()),
            topic_ids: vec![topic1.clone().into_string(), topic2.clone().into_string()],
            signature: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            key: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
        };

        let mut new_message1b = Vec::with_capacity(new_message1.encoded_len());
        new_message1.encode(&mut new_message1b).unwrap();

        let mut old_message1b = Vec::with_capacity(old_message1.encoded_len());
        old_message1.encode(&mut old_message1b).unwrap();

        let mut old_message2b = Vec::with_capacity(old_message2.encoded_len());
        old_message2.encode(&mut old_message2b).unwrap();

        let new_message = super::Message::decode(&old_message1b[..]).unwrap();
        assert_eq!(new_message.topic, topic1.clone().into_string());

        let new_message = super::Message::decode(&old_message2b[..]).unwrap();
        assert_eq!(new_message.topic, topic2.into_string());

        let old_message = compat_proto::Message::decode(&new_message1b[..]).unwrap();
        assert_eq!(old_message.topic_ids, vec![topic1.into_string()]);
    }
}
