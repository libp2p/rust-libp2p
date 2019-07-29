use crate::rpc_proto;
use base64::encode;
use protobuf::Message;
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TopicHash {
    /// The topic hash. Stored as a string to align with the protobuf API.
    hash: String,
}

impl TopicHash {
    pub fn from_raw(hash: impl Into<String>) -> TopicHash {
        TopicHash { hash: hash.into() }
    }

    pub fn into_string(self) -> String {
        self.hash
    }

    pub fn as_str(&self) -> &str {
        &self.hash
    }
}

/// A gossipsub topic.
#[derive(Debug, Clone)]
pub struct Topic {
    topic: String,
}

impl Topic {
    pub fn new(topic: String) -> Self {
        Topic { topic }
    }

    /// Creates a `TopicHash` by SHA256 hashing the topic then base64 encoding the
    /// hash.
    pub fn sha256_hash(&self) -> TopicHash {
        let mut topic_descripter = rpc_proto::TopicDescriptor::new();
        topic_descripter.set_name(self.topic.clone());
        let hash = encode(
            Sha256::digest(
                &topic_descripter
                    .write_to_bytes()
                    .expect("Message is always valid"),
            )
            .as_slice(),
        );

        TopicHash { hash }
    }

    /// Creates a `TopicHash` as a raw string.
    pub fn no_hash(&self) -> TopicHash {
        TopicHash {
            hash: self.topic.clone(),
        }
    }
}

impl fmt::Display for Topic {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.topic)
    }
}

impl fmt::Display for TopicHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.hash)
    }
}
