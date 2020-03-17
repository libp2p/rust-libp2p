// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! A [`ProtocolsHandler`] implementation that combines multiple other `ProtocolsHandler`s
//! indexed by some key.

use crate::NegotiatedSubstream;
use crate::protocols_handler::{
    KeepAlive,
    ProtocolsHandler,
    ProtocolsHandlerEvent,
    ProtocolsHandlerUpgrErr,
    SubstreamProtocol
};
use crate::upgrade::{
    InboundUpgradeSend,
    OutboundUpgradeSend,
    UpgradeInfoSend
};
use futures::{future::BoxFuture, prelude::*};
use libp2p_core::upgrade::ProtocolName;
use rand::Rng;
use std::{collections::HashMap, fmt, hash::Hash, iter::{self, FromIterator}, task::{Context, Poll}};

/// A [`ProtocolsHandler`] for multiple other `ProtocolsHandler`s.
#[derive(Clone)]
pub struct Handler<K, H> {
    handlers: HashMap<K, H>
}

impl<K, H> fmt::Debug for Handler<K, H> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Handler")
    }
}

impl<K, H> Handler<K, H>
where
    K: Clone + std::fmt::Debug + Hash + Eq + Send + 'static,
    H: ProtocolsHandler,
    H::InboundProtocol: InboundUpgradeSend,
    H::OutboundProtocol: OutboundUpgradeSend
{
    /// Create a new empty handler.
    pub fn new() -> Self {
        Handler { handlers: HashMap::new() }
    }

    /// Insert a [`ProtocolsHandler`] at index `key`.
    pub fn add(&mut self, key: K, handler: H) {
        self.handlers.insert(key, handler);
    }

    /// Remove a [`ProtocolsHandler`] at index `key`.
    pub fn del(&mut self, key: &K) {
        self.handlers.remove(key);
    }
}

impl<K, H> FromIterator<(K, H)> for Handler<K, H>
where
    K: Clone + std::fmt::Debug + Hash + Eq + Send + 'static,
    H: ProtocolsHandler,
    H::InboundProtocol: InboundUpgradeSend,
    H::OutboundProtocol: OutboundUpgradeSend
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (K, H)>
    {
        Handler { handlers: HashMap::from_iter(iter) }
    }
}

impl<K, H> ProtocolsHandler for Handler<K, H>
where
    K: Clone + std::fmt::Debug + Hash + Eq + Send + 'static,
    H: ProtocolsHandler,
    H::InboundProtocol: InboundUpgradeSend,
    H::OutboundProtocol: OutboundUpgradeSend
{
    type InEvent = (K, <H as ProtocolsHandler>::InEvent);
    type OutEvent = (K, <H as ProtocolsHandler>::OutEvent);
    type Error = <H as ProtocolsHandler>::Error;
    type InboundProtocol = Upgrade<K, <H as ProtocolsHandler>::InboundProtocol>;
    type OutboundProtocol = <H as ProtocolsHandler>::OutboundProtocol;
    type OutboundOpenInfo = (K, <H as ProtocolsHandler>::OutboundOpenInfo);

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol> {
        let upgrades = self.handlers.iter()
            .map(|(k, h)| (k.clone(), h.listen_protocol().into_upgrade().1))
            .collect();
        SubstreamProtocol::new(Upgrade { upgrades })
    }

    fn inject_fully_negotiated_outbound (
        &mut self,
        protocol: <Self::OutboundProtocol as OutboundUpgradeSend>::Output,
        (key, arg): Self::OutboundOpenInfo
    ) {
        if let Some(h) = self.handlers.get_mut(&key) {
            h.inject_fully_negotiated_outbound(protocol, arg)
        } else {
            log::error!("inject_fully_negotiated_outbound: no handler for key {:?}", key)
        }
    }

    fn inject_fully_negotiated_inbound (
        &mut self,
        (key, arg): <Self::InboundProtocol as InboundUpgradeSend>::Output
    ) {
        if let Some(h) = self.handlers.get_mut(&key) {
            h.inject_fully_negotiated_inbound(arg)
        } else {
            log::error!("inject_fully_negotiated_inbound: no handler for key {:?}", key)
        }
    }

    fn inject_event(&mut self, (key, event): Self::InEvent) {
        if let Some(h) = self.handlers.get_mut(&key) {
            h.inject_event(event)
        } else {
            log::error!("inject_event: no handler for key {:?}", key)
        }
    }

    fn inject_dial_upgrade_error (
        &mut self,
        (key, arg): Self::OutboundOpenInfo,
        error: ProtocolsHandlerUpgrErr<<Self::OutboundProtocol as OutboundUpgradeSend>::Error>
    ) {
        if let Some(h) = self.handlers.get_mut(&key) {
            h.inject_dial_upgrade_error(arg, error)
        } else {
            log::error!("inject_dial_upgrade_error: no handler for protocol {:?}", key)
        }
    }

    fn connection_keep_alive(&self) -> KeepAlive {
        self.handlers.values()
            .map(|h| h.connection_keep_alive())
            .max()
            .unwrap_or(KeepAlive::No)
    }

    fn poll(&mut self, cx: &mut Context)
        -> Poll<ProtocolsHandlerEvent<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::OutEvent, Self::Error>>
    {
        // Not always polling handlers in the same order should give anyone the chance to make progress.
        let pos = rand::thread_rng().gen_range(0, self.handlers.len());

        for (k, h) in self.handlers.iter_mut().skip(pos) {
            if let Poll::Ready(e) = h.poll(cx) {
                let e = e.map_outbound_open_info(|i| (k.clone(), i)).map_custom(|p| (k.clone(), p));
                return Poll::Ready(e)
            }
        }

        for (k, h) in self.handlers.iter_mut().take(pos) {
            if let Poll::Ready(e) = h.poll(cx) {
                let e = e.map_outbound_open_info(|i| (k.clone(), i)).map_custom(|p| (k.clone(), p));
                return Poll::Ready(e)
            }
        }

        Poll::Pending
    }
}

/// Key and protocol name pair used as `UpgradeInfo::Info`.
#[derive(Debug, Clone)]
pub struct KeyedProtoName<K, H>(K, H);

impl<K, H: ProtocolName> ProtocolName for KeyedProtoName<K, H> {
    fn protocol_name(&self) -> &[u8] {
        self.1.protocol_name()
    }
}

/// Inbound and outbound upgrade for all `ProtocolsHandler`s.
#[derive(Clone)]
pub struct Upgrade<K, H> {
    upgrades: HashMap<K, H>
}

impl<K, H> fmt::Debug for Upgrade<K, H> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("Upgrade")
    }
}

impl<K, H> UpgradeInfoSend for Upgrade<K, H>
where
    K: Hash + Eq + Clone + Send + 'static,
    H: UpgradeInfoSend
{
    type Info = KeyedProtoName<K, H::Info>;
    type InfoIter = std::vec::IntoIter<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        self.upgrades.iter().map(|(k, i)| iter::repeat(k.clone()).zip(i.protocol_info()))
            .flatten()
            .map(|(k, i)| KeyedProtoName(k, i))
            .collect::<Vec<_>>()
            .into_iter()
    }
}

impl<K, H> InboundUpgradeSend for Upgrade<K, H>
where
    H: InboundUpgradeSend,
    K: Clone + Hash + Eq + Send + 'static
{
    type Output = (K, <H as InboundUpgradeSend>::Output);
    type Error  = (K, <H as InboundUpgradeSend>::Error);
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_inbound(mut self, resource: NegotiatedSubstream, info: Self::Info) -> Self::Future {
        let KeyedProtoName(key, info) = info;
        let u = self.upgrades.remove(&key).expect(
            "`upgrade_inbound` is applied to a key from `protocol_info`, which only contains \
            keys from the same set of upgrades we are searching here, therefore looking for this \
            key is guaranteed to give us a non-empty result; qed"
        );
        u.upgrade_inbound(resource, info).map(move |out| {
            match out {
                Ok(o) => Ok((key, o)),
                Err(e) => Err((key, e))
            }
        })
        .boxed()
    }
}

impl<K, H> OutboundUpgradeSend for Upgrade<K, H>
where
    H: OutboundUpgradeSend,
    K: Clone + Hash + Eq + Send + 'static
{
    type Output = (K, <H as OutboundUpgradeSend>::Output);
    type Error  = (K, <H as OutboundUpgradeSend>::Error);
    type Future = BoxFuture<'static, Result<Self::Output, Self::Error>>;

    fn upgrade_outbound(mut self, resource: NegotiatedSubstream, info: Self::Info) -> Self::Future {
        let KeyedProtoName(key, info) = info;
        let u = self.upgrades.remove(&key).expect(
            "`upgrade_outbound` is applied to a key from `protocol_info`, which only contains \
            keys from the same set of upgrades we are searching here, therefore looking for this \
            key is guaranteed to give us a non-empty result; qed"
        );
        u.upgrade_outbound(resource, info).map(move |out| {
            match out {
                Ok(o) => Ok((key, o)),
                Err(e) => Err((key, e))
            }
        })
        .boxed()
    }
}

