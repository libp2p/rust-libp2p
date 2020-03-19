/*
 * Copyright 2019 Fluence Labs Limited
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use crate::Addresses;
use libp2p_core::{Multiaddr};
use libp2p_core::identity::ed25519::PublicKey;
use crate::protocol::KadPeer;
use smallvec::SmallVec;
use std::fmt::Formatter;
use bs58;

#[derive(Clone, PartialEq, Eq)]
pub struct Contact {
    pub addresses: Addresses,
    pub public_key: PublicKey
}

impl Contact {
    pub fn new(addresses: Addresses, public_key: PublicKey) -> Self {
        Self {
            addresses,
            public_key
        }
    }

    pub fn first(&self) -> &Multiaddr {
        self.addresses.first()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Multiaddr> {
        self.addresses.iter()
    }
    pub fn len(&self) -> usize {
        self.addresses.len()
    }
    pub fn into_vec(self) -> Vec<Multiaddr> {
        self.addresses.into_vec()
    }
    pub fn insert(&mut self, addr: Multiaddr) -> bool {
        self.addresses.insert(addr)
    }
}

impl Into<Addresses> for Contact {
    fn into(self) -> Addresses {
        self.addresses
    }
}

impl From<KadPeer> for Contact {
    fn from(peer: KadPeer) -> Self {
        Contact {
            addresses: peer.multiaddrs.iter().cloned().collect::<SmallVec<[Multiaddr; 6]>>().into(),
            public_key: peer.public_key
        }
    }
}

impl std::fmt::Display for Contact {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
           "Contact({}, addresses: {:?})",
            bs58::encode(self.public_key.encode()).into_string(),
            self.addresses // TODO: implement better display for addresses
        )
    }
}

impl std::fmt::Debug for Contact {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Contact {{ public_key: {}, addresses: {:?} }}",
            bs58::encode(self.public_key.encode()).into_string(),
            self.addresses // TODO: implement better display for addresses
        )
    }
}