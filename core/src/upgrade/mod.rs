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

mod apply;
mod denied;
mod error;
mod map;
mod or;
mod toggleable;

use bytes::Bytes;
use futures::future::Future;

pub use self::{
    apply::{apply_inbound, apply_outbound, InboundUpgradeApply, OutboundUpgradeApply},
    denied::DeniedUpgrade,
    error::UpgradeError,
    map::{MapUpgrade, MapUpgradeErr},
    or::OrUpgrade,
    toggleable::{toggleable, Toggleable}
};

pub trait UpgradeInfo {
    type UpgradeId;
    type NamesIter: Iterator<Item = (Bytes, Self::UpgradeId)>;

    fn protocol_names(&self) -> Self::NamesIter;
}

pub trait InboundUpgrade<C>: UpgradeInfo {
    type Output;
    type Error;
    type Future: Future<Item = Self::Output, Error = Self::Error>;

    fn upgrade_inbound(self, socket: C, id: Self::UpgradeId) -> Self::Future;
}

pub trait InboundUpgradeExt<C>: InboundUpgrade<C> {
    fn map_inbound<F, T>(self, f: F) -> MapUpgrade<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output) -> T
    {
        MapUpgrade::new(self, f)
    }

    fn map_inbound_err<F, T>(self, f: F) -> MapUpgradeErr<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error) -> T
    {
        MapUpgradeErr::new(self, f)
    }

    fn or_inbound<U>(self, upgrade: U) -> OrUpgrade<Self, U>
    where
        Self: Sized,
        U: InboundUpgrade<C, Output = Self::Output, Error = Self::Error>
    {
        OrUpgrade::new(self, upgrade)
    }
}

impl<C, U: InboundUpgrade<C>> InboundUpgradeExt<C> for U {}

pub trait OutboundUpgrade<C>: UpgradeInfo {
    type Output;
    type Error;
    type Future: Future<Item = Self::Output, Error = Self::Error>;

    fn upgrade_outbound(self, socket: C, id: Self::UpgradeId) -> Self::Future;
}

pub trait OutboundUpgradeExt<C>: OutboundUpgrade<C> {
    fn map_outbound<F, T>(self, f: F) -> MapUpgrade<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Output) -> T
    {
        MapUpgrade::new(self, f)
    }

    fn map_outbound_err<F, T>(self, f: F) -> MapUpgradeErr<Self, F>
    where
        Self: Sized,
        F: FnOnce(Self::Error) -> T
    {
        MapUpgradeErr::new(self, f)
    }

    fn or_outbound<U>(self, upgrade: U) -> OrUpgrade<Self, U>
    where
        Self: Sized,
        U: OutboundUpgrade<C, Output = Self::Output, Error = Self::Error>
    {
        OrUpgrade::new(self, upgrade)
    }
}

impl<C, U: OutboundUpgrade<C>> OutboundUpgradeExt<C> for U {}

