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

use crate::rpc_proto;
use base64::encode;
use prometheus_client::encoding::text::Encode;
use prost::Message;
use sha2::{Digest, Sha256};
use std::fmt;

/// A generic trait that can be extended for various hashing types for a topic.
pub trait Hasher {
    /// The function that takes a topic string and creates a topic hash.
    fn hash(topic_string: String) -> TopicHash;
}

/// A type for representing topics who use the identity hash.
#[derive(Debug, Clone)]
pub struct IdentityHash {}
impl Hasher for IdentityHash {
    /// Creates a [`TopicHash`] as a raw string.
    fn hash(topic_string: String) -> TopicHash {
        TopicHash { hash: topic_string }
    }
}

#[derive(Debug, Clone)]
pub struct Sha256Hash {}
impl Hasher for Sha256Hash {
    /// Creates a [`TopicHash`] by SHA256 hashing the topic then base64 encoding the
    /// hash.
    fn hash(topic_string: String) -> TopicHash {
        let topic_descripter = rpc_proto::TopicDescriptor {
            name: Some(topic_string),
            auth: None,
            enc: None,
        };
        let mut bytes = Vec::with_capacity(topic_descripter.encoded_len());
        topic_descripter
            .encode(&mut bytes)
            .expect("buffer is large enough");
        let hash = encode(Sha256::digest(&bytes).as_slice());
        TopicHash { hash }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Encode)]
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Topic<H: Hasher> {
    topic: String,
    phantom_data: std::marker::PhantomData<H>,
}

impl<H: Hasher> From<Topic<H>> for TopicHash {
    fn from(topic: Topic<H>) -> TopicHash {
        topic.hash()
    }
}

impl<H: Hasher> Topic<H> {
    pub fn new(topic: impl Into<String>) -> Self {
        Topic {
            topic: topic.into(),
            phantom_data: std::marker::PhantomData,
        }
    }

    pub fn hash(&self) -> TopicHash {
        H::hash(self.topic.clone())
    }
}

impl<H: Hasher> fmt::Display for Topic<H> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.topic)
    }
}

impl fmt::Display for TopicHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.hash)
    }
}
