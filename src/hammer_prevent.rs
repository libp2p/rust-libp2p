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

use crate::{Multiaddr, core::Transport, core::transport::TransportError};
use fnv::FnvHashMap;
use futures::{prelude::*, future, stream};
use parking_lot::Mutex;
use std::{collections::hash_map::Entry, cmp, error, fmt, mem, sync::Arc, time::Duration, time::Instant};

/// First value for the back-off.
const FIRST_BACKOFF: Duration = Duration::from_secs(2);
/// Maximum allowed backoff.
const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// Wraps around a `Transport`. Remembers addresses that couldn't be dialed and avoids dialing them
/// again.
#[derive(Clone)]
pub struct HammerPrevention<TInner> {
    /// The underlying transport.
    inner: TInner,
    /// For each address, when we can dial it again.
    addresses: Arc<Mutex<FnvHashMap<Multiaddr, AddrInfo>>>,
}

struct AddrInfo {
    /// If we try to dial this address, how much delay we should add.
    next_dial_backoff: Duration,
    /// When we can remove this address from the list.
    expiration: Instant,
}

impl<TInner> HammerPrevention<TInner> {
    /// Creates a new `HammerPrevention` around the transport.
    pub fn new(inner: TInner) -> Self {
        HammerPrevention {
            inner,
            addresses: Arc::new(Mutex::new(Default::default())),
        }
    }
}

impl<TInner> Transport for HammerPrevention<TInner>
where
    TInner: Transport,
    TInner::Error: 'static,
{
    type Output = TInner::Output;
    type Error = HammerPreventionErr<TInner::Error>;
    type Listener = stream::MapErr<stream::Map<TInner::Listener, fn((TInner::ListenerUpgrade, Multiaddr)) -> (Self::ListenerUpgrade, Multiaddr)>, fn(TInner::Error) -> HammerPreventionErr<TInner::Error>>;
    type ListenerUpgrade = future::MapErr<TInner::ListenerUpgrade, fn(TInner::Error) -> HammerPreventionErr<TInner::Error>>;
    type Dial = HammerPreventionDial<TInner>;

    fn listen_on(self, addr: Multiaddr) -> Result<(Self::Listener, Multiaddr), TransportError<Self::Error>> {
        // The entire code of this method is just about converting to the proper error types.
        let (listen, new_addr) = self.inner.listen_on(addr)
            .map_err(|err| {
                match err {
                    TransportError::MultiaddrNotSupported(addr) => TransportError::MultiaddrNotSupported(addr),
                    TransportError::Other(err) => TransportError::Other(HammerPreventionErr::Inner(TransportError::Other(err))),
                }
            })?;

        let listen = listen
            .map::<_, fn(_) -> _>(|(upgr, addr)| {
                (upgr.map_err::<fn(_) -> _, _>(|err| HammerPreventionErr::Inner(TransportError::Other(err))), addr)
            })
            .map_err::<_, fn(_) -> _>(|err| HammerPreventionErr::Inner(TransportError::Other(err)));

        Ok((listen, new_addr))
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        let mut addresses = self.addresses.lock();

        // Purge expired addresses.
        let now = Instant::now();
        addresses.retain(move |_, a| a.expiration >= now);

        let inner = match addresses.entry(addr.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(AddrInfo {
                    next_dial_backoff: FIRST_BACKOFF,
                    expiration: Instant::now() + FIRST_BACKOFF * 2,
                });

                // Dial immediately.
                match self.inner.dial(addr.clone()) {
                    Ok(f) => HammerPreventionDialInner::Dialing(f),
                    Err(TransportError::MultiaddrNotSupported(addr)) =>
                        return Err(TransportError::MultiaddrNotSupported(addr)),
                    Err(TransportError::Other(err)) =>
                        return Err(TransportError::Other(HammerPreventionErr::Inner(TransportError::Other(err))))
                }
            }

            Entry::Occupied(entry) => {
                // Adding a waiting delay before the dial.
                let addr_info = entry.into_mut();
                let timer = tokio_timer::Delay::new(Instant::now() + addr_info.next_dial_backoff);
                addr_info.next_dial_backoff = cmp::min(addr_info.next_dial_backoff * 2, MAX_BACKOFF);
                addr_info.expiration = Instant::now() + addr_info.next_dial_backoff * 2;
                HammerPreventionDialInner::Waiting(timer, self.inner)
            }
        };

        Ok(HammerPreventionDial {
            inner,
            local_addr: addr,
            addresses: self.addresses.clone(),
        })
    }

    fn nat_traversal(&self, server: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.nat_traversal(server, observed)
    }
}

/// Wraps around a `Future` that produces a connection. If necessary, waits for a little bit
/// before dialing.
pub struct HammerPreventionDial<TInner>
where
    TInner: Transport
{
    /// Inner state of the future.
    inner: HammerPreventionDialInner<TInner>,
    /// Address we're trying to dial.
    local_addr: Multiaddr,
    /// The shared state containing the back-off for the addresses.
    addresses: Arc<Mutex<FnvHashMap<Multiaddr, AddrInfo>>>,
}

enum HammerPreventionDialInner<TInner>
where
    TInner: Transport
{
    /// We are during the dialing delay.
    Waiting(tokio_timer::Delay, TInner),
    /// We are in the process of dialing.
    Dialing(TInner::Dial),
    /// Temporary poisoned state. If encountered is poisoned, meaning that there's either a bug in
    /// the state machine or that the future has ended.
    Poisoned,
}

impl<TInner> Future for HammerPreventionDial<TInner>
where
    TInner: Transport,
{
    type Item = TInner::Output;
    type Error = HammerPreventionErr<TInner::Error>;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            match mem::replace(&mut self.inner, HammerPreventionDialInner::Poisoned) {
                HammerPreventionDialInner::Waiting(mut delay, transport) => {
                    match delay.poll() {
                        Ok(Async::Ready(())) => {
                            match transport.dial(self.local_addr.clone()) {
                                Ok(f) => self.inner = HammerPreventionDialInner::Dialing(f),
                                Err(err) => return Err(HammerPreventionErr::Inner(err))
                            }
                        },
                        Ok(Async::NotReady) => {
                            self.inner = HammerPreventionDialInner::Waiting(delay, transport);
                            return Ok(Async::NotReady)
                        },
                        Err(err) => return Err(HammerPreventionErr::Timer(err)),
                    }
                }

                HammerPreventionDialInner::Dialing(mut dial) => {
                    match dial.poll() {
                        Ok(Async::Ready(output)) => {
                            // On success, remove the address from the list.
                            let _ = self.addresses.lock().remove(&self.local_addr);
                            return Ok(Async::Ready(output))
                        }
                        Ok(Async::NotReady) => {
                            self.inner = HammerPreventionDialInner::Dialing(dial);
                            return Ok(Async::NotReady)
                        },
                        Err(err) => return Err(HammerPreventionErr::Inner(TransportError::Other(err))),
                    }
                }

                HammerPreventionDialInner::Poisoned => {
                    panic!("Polled the HammerPreventionDial after it was finished")
                }
            }
        }
    }
}

/// Possible errors when dialing.
#[derive(Debug)]
pub enum HammerPreventionErr<TInner> {
    /// Error in the inner transport.
    Inner(TransportError<TInner>),

    /// Error in the tokio timer.
    Timer(tokio_timer::Error),
}

impl<TInner> fmt::Display for HammerPreventionErr<TInner>
where
    TInner: error::Error,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HammerPreventionErr::Inner(err) => fmt::Display::fmt(err, f),
            HammerPreventionErr::Timer(err) => fmt::Display::fmt(err, f),
        }
    }
}

impl<TInner> error::Error for HammerPreventionErr<TInner>
where
    TInner: error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            HammerPreventionErr::Inner(err) => Some(err),
            HammerPreventionErr::Timer(err) => Some(err),
        }
    }
}
