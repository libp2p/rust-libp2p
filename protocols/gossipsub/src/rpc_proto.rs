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

pub(crate) mod proto {
    #![allow(unreachable_pub)]
    include!("generated/mod.rs");
    pub use self::gossipsub::pb::{mod_RPC::SubOpts, *};
}

#[cfg(test)]
mod test {
    use crate::rpc_proto::proto::compat;
    use crate::IdentTopic as Topic;
    use libp2p_identity::PeerId;
    use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
    use rand::Rng;

    #[test]
    fn test_multi_topic_message_compatibility() {
        let topic1 = Topic::new("t1").hash();
        let topic2 = Topic::new("t2").hash();

        let new_message1 = super::proto::Message {
            from: Some(PeerId::random().to_bytes()),
            data: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            seqno: Some(rand::thread_rng().gen::<[u8; 8]>().to_vec()),
            topic: topic1.clone().into_string(),
            signature: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            key: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
        };
        let old_message1 = compat::pb::Message {
            from: Some(PeerId::random().to_bytes()),
            data: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            seqno: Some(rand::thread_rng().gen::<[u8; 8]>().to_vec()),
            topic_ids: vec![topic1.clone().into_string()],
            signature: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            key: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
        };
        let old_message2 = compat::pb::Message {
            from: Some(PeerId::random().to_bytes()),
            data: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            seqno: Some(rand::thread_rng().gen::<[u8; 8]>().to_vec()),
            topic_ids: vec![topic1.clone().into_string(), topic2.clone().into_string()],
            signature: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
            key: Some(rand::thread_rng().gen::<[u8; 32]>().to_vec()),
        };

        let mut new_message1b = Vec::with_capacity(new_message1.get_size());
        let mut writer = Writer::new(&mut new_message1b);
        new_message1.write_message(&mut writer).unwrap();

        let mut old_message1b = Vec::with_capacity(old_message1.get_size());
        let mut writer = Writer::new(&mut old_message1b);
        old_message1.write_message(&mut writer).unwrap();

        let mut old_message2b = Vec::with_capacity(old_message2.get_size());
        let mut writer = Writer::new(&mut old_message2b);
        old_message2.write_message(&mut writer).unwrap();

        let mut reader = BytesReader::from_bytes(&old_message1b[..]);
        let new_message =
            super::proto::Message::from_reader(&mut reader, &old_message1b[..]).unwrap();
        assert_eq!(new_message.topic, topic1.clone().into_string());

        let mut reader = BytesReader::from_bytes(&old_message2b[..]);
        let new_message =
            super::proto::Message::from_reader(&mut reader, &old_message2b[..]).unwrap();
        assert_eq!(new_message.topic, topic2.into_string());

        let mut reader = BytesReader::from_bytes(&new_message1b[..]);
        let old_message =
            compat::pb::Message::from_reader(&mut reader, &new_message1b[..]).unwrap();
        assert_eq!(old_message.topic_ids, vec![topic1.into_string()]);
    }
}
