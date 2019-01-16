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
use std::{
    collections::{HashMap,
        hash_map::{IntoIter, Iter, Values, Keys}
    },
    hash::{Hash, Hasher},
    iter::FromIterator,
};

/// Used in `GMessage`, thus the `Hash` derive is required.
///
/// Seems like PartialEq, Eq and Hash need to be implemented manually, since
/// Compiler errors result if they are just derived (e.g. when used in
/// `CacheEntry`). Not sure why Hash can't be derived (I get a compiler error
/// for `HashMap`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicMap(HashMap<TopicHash, Topic>);

impl TopicMap {
    pub fn new() -> TopicMap {
        TopicMap(HashMap::new())
    }

    pub fn insert(&mut self, tr: TopicHash, t: Topic) -> Option<Topic> {
        self.0.insert(tr, t)
    }

    pub fn values(&self) -> Values<TopicHash, Topic> {
        self.0.values()
    }

    pub fn iter(&self) -> Iter<TopicHash, Topic> {
        self.0.iter()
    }

    pub fn keys(&self) -> Keys<TopicHash, Topic> {
        self.0.keys()
    }
}

impl FromIterator<TopicHash> for TopicMap {
    fn from_iter<I: IntoIterator<Item=TopicHash>>(iter: I) -> Self {
        let mut tm = TopicMap::new();

        for tr in iter {
            let t = Topic::from(&tr);
            tm.insert(tr, t);
        }

        tm
    }
}

impl IntoIterator for TopicMap {
    type Item = (TopicHash, Topic);
    type IntoIter = IntoIter<TopicHash, Topic>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl FromIterator<Topic> for TopicMap {
    fn from_iter<I: IntoIterator<Item=Topic>>(iter: I) -> Self {
        let mut tm = TopicMap::new();

        for t in iter {
            let th = TopicHash::from(&t);
            tm.insert(th, t);
        }

        tm
    }
}

/// Represents the hash of a topic.
///
/// Instead of using the topic as a whole, the API of gossipsub uses a hash of
/// the topic. You only have to build the hash once, then use it everywhere.
/// Needs to derive `Eq` and `Hash` e.g. because it is used as a key in
/// `HashMap` of `TopicMap`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TopicHash {
    hash: String,
}

impl TopicHash {
    pub(crate) fn new() -> Self {
        TopicHash { hash: String::new() }
    }

    /// Builds a new `TopicHash` from the given hash.
    // TODO: check if the TopicHash matches that of an existing `Topic`?
    // Use impl From<TopicHash> for Topic. Would need to search through topics
    // in the network until one is found, or return an error.
    // Not sure if there would be any use case for constructing a new `Topic`
    // from a raw `String` (via a `TopicHash`).
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

impl From<Topic> for TopicHash {
    #[inline]
    fn from(topic: Topic) -> Self {
        topic.hash
    }
}

impl<'a> From<&'a Topic> for TopicHash {
    #[inline]
    fn from(topic: &'a Topic) -> Self {
        topic.hash.clone()
    }
}

/// Built topic.
#[derive(Debug, Clone)]
pub struct Topic {
    descriptor: rpc_proto::TopicDescriptor,
    hash: TopicHash,
}

impl PartialEq for Topic {
    fn eq(&self, other: &Topic) -> bool {
        self.hash == other.hash
    }
}
impl Eq for Topic {}

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

// TODO: test
impl From<TopicHash> for Topic {
    fn from(topic_hash: TopicHash) -> Self {
        let t_hash = topic_hash.clone();
        let vec_from_hash = bs58::decode(topic_hash.hash).into_vec().unwrap();
        let bytes_from_hash: &[u8] = vec_from_hash.as_ref();
        let descriptor =
            protobuf::parse_from_bytes::<rpc_proto::TopicDescriptor>(
                bytes_from_hash).unwrap();
        Topic {
            descriptor: descriptor,
            hash: t_hash,
        }
    }
}

impl<'a> From<&'a TopicHash> for Topic {
    fn from(topic_hash: &'a TopicHash) -> Self {
        let t_hash = topic_hash.clone();
        let vec_from_hash = bs58::decode(topic_hash.clone().hash).into_vec().unwrap();
        let bytes_from_hash: &[u8] = vec_from_hash.as_ref();
        let descriptor =
            protobuf::parse_from_bytes::<rpc_proto::TopicDescriptor>(
                bytes_from_hash).unwrap();
        Topic {
            descriptor: descriptor,
            hash: t_hash,
        }
    }
}

// impl From<TopicRep> for Topic {
//     fn from(topic_rep: TopicRep) -> Self {
//         match topic_rep {
//             TopicRep::Hash(TopicHash) => Topic::from(topic_rep),
//             TopicRep::Id(TopicId) => Topic::from(topic_rep),
//         }
//     }
// }

impl<'a> From<&'a TopicRep> for Topic {
    fn from(topic_rep: &'a TopicRep) -> Self {
        match topic_rep {
            TopicRep::Hash(TopicHash) => Topic::from(topic_rep),
            TopicRep::Id(TopicId) => Topic::from(topic_rep),
        }
    }
}

// Nightly experimental API.
// https://github.com/rust-lang/rust/issues/33417
// impl TryFrom<TopicId> for Topic {
    // Unlike a `TopicHash`, we can't rebuild a `Topic` from a `TopicId`,
    // we have to fetch it from somewhere it is stored. In this context,
    // this would be the `MCache`, although messages are only stored for a
    // few seconds / heartbeat intervals, hence implementing `Try` won't work.
//     fn try_from(t_id: TopicId) -> Result<Self, Self::Error> {

//     }
// }

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

/// Represents a `Topic` via either a `TopicHash` or a `TopicId`.
/// Due to the added difficulty of converting a `TopicId` to a `Topic`
/// it is suggested to just use a `TopicHash`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TopicRep {
    Hash(TopicHash),
    Id(TopicId)
}

impl From<TopicHash> for TopicRep {
    fn from(topic_hash: TopicHash) -> Self {
        TopicRep::Hash(topic_hash)
    }
}

impl From<TopicId> for TopicRep {
    fn from(topic_id: TopicId) -> Self {
        TopicRep::Id(topic_id)
    }
}

/// Contains a string that can be used to query for and thus represent a
/// `Topic`.
/// Not used
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

#[cfg(test)]
mod tests {

}
