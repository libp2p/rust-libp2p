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

use rpc_proto;

use bs58;
use protobuf::Message;
use std::collections::hash_map::HashMap;
use std::iter::Map;

pub type TopicHashMap = HashMap<TopicHash, Topic>;
pub type TopicIdMap = HashMap<TopicId, Topic>;
// pub type TopicMap = HashMap<TopicRep, Topic>;

#[derive(Debug)]
pub struct TopicMap(Map<TopicRep, Topic>);

impl TopicMap {
    fn new() -> TopicMap {
        TopicMap(Map::new())
    }

    fn insert(&mut self, tr: TopicRep, t: Topic) -> Option<Topic> {
        self.0.insert(tr, t)
    }
}

impl std::iter::FromIterator<TopicHash> for TopicMap {
    fn from_iter<I: IntoIterator<Item=TopicHash>>(iter: I) -> Self {
        let mut tm = TopicMap::new();

        for i in iter {
            let tr = TopicRep::Hash(i);
            let t = 
            tm.insert(tr, t);
        }

        tm
    }
}

// #[derive(Debug)]
// pub struct TopicMap(HashMap<TopicRep, Topic>);

// impl TopicMap {
//     fn new() -> TopicMap {
//         TopicMap(HashMap::new())
//     }

//     fn insert(&mut self, tr: TopicRep, t: Topic) -> Option<Topic> {
//         self.0.insert(tr, t)
//     }
// }

// impl std::iter::FromIterator<TopicHash> for TopicMap {
//     fn from_iter<I: IntoIterator<Item=TopicHash>>(iter: I) -> Self {
//         let mut tm = TopicMap::new();

//         for i in iter {
//             let tr = TopicRep::Hash(i);
//             let t = 
//             tm.insert(tr, t);
//         }

//         tm
//     }
// }

/// Represents a `Topic` via either a `TopicHash` or a `TopicId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TopicRep {
    Hash(TopicHash),
    Id(TopicId)
}

// TODO
// impl From<TopicRep> for Topic {
//     pub fn from(topic_rep: TopicRep) -> Topic {
//         for topic in 
//     }
// }

/// Represents the hash of a topic.
///
/// Instead of using the topic as a whole, the API of floodsub uses a hash of
/// the topic. You only have to build the hash once, then use it everywhere.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TopicHash {
    hash: String,
}

impl TopicHash {
    /// Builds a new `TopicHash` from the given hash.
    #[inline]
    pub fn from_raw(hash: String) -> TopicHash {
        TopicHash { hash: hash }
    }

    /// Converts the `TopicHash` into a raw hash `String`.
    #[inline]
    pub fn into_string(self) -> String {
        self.hash
    }
}

// TODO
// impl From<TopicHash> for Topic {
//     fn from(topic_hash: TopicHash) -> Topic {
        
//     }
// }

impl From<TopicHash> for TopicRep {
    fn from(topic_hash: TopicHash) -> TopicRep {
        TopicRep::Hash(topic_hash);
    }
}

/// Built topic.
#[derive(Debug, Clone, PartialEq, Eq)]
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

        TopicBuilder { builder: builder }
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

/// Contains a string that can be used to query for and thus represent a
/// `Topic`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TopicId {
    id: String,
}

impl TopicId {
    pub fn new(s: &str) -> Self {
        TopicId {
            id: s.to_owned(),
        }
    }
}

impl From<TopicId> for TopicRep {
    fn from(topic_id: TopicId) -> TopicRep {
        TopicRep::Id(topic_id)
    }
}