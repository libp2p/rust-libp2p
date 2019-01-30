// Copyright 2018 Parity Technologies (UK) Ltd.
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

use bs58;
use crate::rpc_proto;
use protobuf::Message;

/// Represents the hash of a topic.
///
/// Instead of a using the topic as a whole, the API of floodsub uses a hash of the topic. You only
/// have to build the hash once, then use it everywhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TopicHash {
    hash: String,
}

impl TopicHash {
    /// Builds a new `TopicHash` from the given hash.
    #[inline]
    pub fn from_raw(hash: String) -> TopicHash {
        TopicHash { hash }
    }

    #[inline]
    pub fn into_string(self) -> String {
        self.hash
    }
}

/// Built topic.
#[derive(Debug, Clone)]
pub struct Topic {
    descriptor: rpc_proto::TopicDescriptor,
    hash: TopicHash,
}

impl Topic {
    /// Returns the hash of the topic.
    #[inline]
    pub fn hash(&self) -> &TopicHash {
        &self.hash
    }
}

impl AsRef<TopicHash> for Topic {
    #[inline]
    fn as_ref(&self) -> &TopicHash {
        &self.hash
    }
}

impl From<Topic> for TopicHash {
    #[inline]
    fn from(topic: Topic) -> TopicHash {
        topic.hash
    }
}

impl<'a> From<&'a Topic> for TopicHash {
    #[inline]
    fn from(topic: &'a Topic) -> TopicHash {
        topic.hash.clone()
    }
}

/// Builder for a `TopicHash`.
#[derive(Debug, Clone)]
pub struct TopicBuilder {
    builder: rpc_proto::TopicDescriptor,
}

impl TopicBuilder {
    pub fn new<S>(name: S) -> TopicBuilder
    where
        S: Into<String>,
    {
        let mut builder = rpc_proto::TopicDescriptor::new();
        builder.set_name(name.into());

        TopicBuilder { builder }
    }

    /// Turns the builder into an actual `Topic`.
    pub fn build(self) -> Topic {
        let bytes = self
            .builder
            .write_to_bytes()
            .expect("protobuf message is always valid");
        // TODO: https://github.com/libp2p/rust-libp2p/issues/473
        let hash = TopicHash {
            hash: bs58::encode(&bytes).into_string(),
        };
        Topic {
            descriptor: self.builder,
            hash,
        }
    }
}
