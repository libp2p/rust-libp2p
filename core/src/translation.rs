// Copyright 2019 Parity Technologies (UK) Ltd.
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

use multiaddr::{Multiaddr, Protocol};

/// Perform IP address translation.
///
/// Given an `original` [`Multiaddr`] and some `observed` [`Multiaddr`], return
/// a translated [`Multiaddr`] which has the first IP address translated by the
/// corresponding one from `observed`.
///
/// This is a mixed-mode translation, i.e. an IPv4 address may be replaced by
/// an IPv6 address and vice versa.
///
/// If the first [`Protocol`]s are not IP addresses, `None` is returned instead.
pub fn address_translation(original: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
    original.replace(0, move |proto| match proto {
        Protocol::Ip4(_) | Protocol::Ip6(_) => match observed.iter().next() {
            x@Some(Protocol::Ip4(_)) => x,
            x@Some(Protocol::Ip6(_)) => x,
            _ => None
        }
        _ => None
    })
}
// TODO: add tests
