// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! Implementation of the `Peerstore` trait that uses a single JSON file as backend.

use super::TTL;
use bs58;
use datastore::{Datastore, JsonFileDatastore, JsonFileDatastoreEntry, Query};
use futures::{Future, Stream};
use multiaddr::Multiaddr;
use peer_info::{AddAddrBehaviour, PeerInfo};
use peerstore::{PeerAccess, Peerstore};
use std::io::Error as IoError;
use std::iter;
use std::path::PathBuf;
use std::vec::IntoIter as VecIntoIter;
use PeerId;

/// Peerstore backend that uses a Json file.
pub struct JsonPeerstore {
    store: JsonFileDatastore<PeerInfo>,
}

impl JsonPeerstore {
    /// Opens a new peerstore tied to a JSON file at the given path.
    ///
    /// If the file exists, this function will open it. In any case, flushing the peerstore or
    /// destroying it will write to the file.
    #[inline]
    pub fn new<P>(path: P) -> Result<JsonPeerstore, IoError>
    where
        P: Into<PathBuf>,
    {
        Ok(JsonPeerstore {
            store: JsonFileDatastore::new(path)?,
        })
    }

    /// Flushes the content of the peer store to the disk.
    ///
    /// This function can only fail in case of a disk access error. If an error occurs, any change
    /// to the peerstore that was performed since the last successful flush will be lost. No data
    /// will be corrupted.
    #[inline]
    pub fn flush(&self) -> Result<(), IoError> {
        self.store.flush()
    }
}

impl<'a> Peerstore for &'a JsonPeerstore {
    type PeerAccess = JsonPeerstoreAccess<'a>;
    type PeersIter = Box<Iterator<Item = PeerId>>;

    #[inline]
    fn peer(self, peer_id: &PeerId) -> Option<Self::PeerAccess> {
        let hash = bs58::encode(peer_id.as_bytes()).into_string();
        self.store.lock(hash.into()).map(JsonPeerstoreAccess)
    }

    #[inline]
    fn peer_or_create(self, peer_id: &PeerId) -> Self::PeerAccess {
        let hash = bs58::encode(peer_id.as_bytes()).into_string();
        JsonPeerstoreAccess(self.store.lock_or_create(hash.into()))
    }

    fn peers(self) -> Self::PeersIter {
        let query = self.store.query(Query {
            prefix: "".into(),
            filters: vec![],
            orders: vec![],
            skip: 0,
            limit: u64::max_value(),
            keys_only: true,
        });

        let list = query
            .filter_map(|(key, info)| {
                if info.addrs().count() == 0 {
                    return None // all addresses are expired
                }
                // We filter out invalid elements. This can happen if the JSON storage file was
                // corrupted or manually modified by the user.
                PeerId::from_bytes(bs58::decode(key).into_vec().ok()?).ok()
            })
            .collect()
            .wait(); // Wait can never block for the JSON datastore.

        // Need to handle I/O errors. Again we just ignore.
        if let Ok(list) = list {
            Box::new(list.into_iter()) as Box<_>
        } else {
            Box::new(iter::empty()) as Box<_>
        }
    }
}

pub struct JsonPeerstoreAccess<'a>(JsonFileDatastoreEntry<'a, PeerInfo>);

impl<'a> PeerAccess for JsonPeerstoreAccess<'a> {
    type AddrsIter = VecIntoIter<Multiaddr>;

    #[inline]
    fn addrs(&self) -> Self::AddrsIter {
        self.0.addrs().cloned().collect::<Vec<_>>().into_iter()
    }

    #[inline]
    fn add_addr(&mut self, addr: Multiaddr, ttl: TTL) {
        self.0
            .add_addr(addr, ttl, AddAddrBehaviour::IgnoreTtlIfInferior);
    }

    #[inline]
    fn set_addr_ttl(&mut self, addr: Multiaddr, ttl: TTL) {
        self.0.add_addr(addr, ttl, AddAddrBehaviour::OverwriteTtl);
    }

    #[inline]
    fn clear_addrs(&mut self) {
        self.0.set_addrs(iter::empty());
    }
}

#[cfg(test)]
mod tests {
    extern crate tempfile;
    peerstore_tests!(
        {::json_peerstore::JsonPeerstore::new(temp_file.path()).unwrap()}
        {let temp_file = self::tempfile::NamedTempFile::new().unwrap()}
    );

    #[test]
    fn reload() {
        let temp_file = self::tempfile::NamedTempFile::new().unwrap();
        let peer_store = ::json_peerstore::JsonPeerstore::new(temp_file.path()).unwrap();

        let peer_id = PeerId::from_public_key(PublicKey::Ed25519(vec![1, 2, 3]));
        let addr = "/ip4/0.0.0.0/tcp/0".parse::<Multiaddr>().unwrap();

        peer_store
            .peer_or_create(&peer_id)
            .add_addr(addr.clone(), Duration::from_millis(5000));
        peer_store.flush().unwrap();
        drop(peer_store);

        let peer_store = ::json_peerstore::JsonPeerstore::new(temp_file.path()).unwrap();
        let addrs = peer_store
            .peer(&peer_id)
            .unwrap()
            .addrs()
            .collect::<Vec<_>>();
        assert_eq!(addrs, &[addr]);
    }
}
