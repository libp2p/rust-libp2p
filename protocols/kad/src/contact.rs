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

// TODO: modify src/dht.proto


use crate::Addresses;
use libp2p_core::{Multiaddr};
use libp2p_core::identity::ed25519::PublicKey;

#[derive(Clone)]
pub struct Contact {
    pub addresses: Addresses,
    pub public_key: PublicKey
}

impl Contact {
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
    pub fn remove(&mut self, addr: &Multiaddr) -> Result<(),()> {
        self.addresses.remove(addr)
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
