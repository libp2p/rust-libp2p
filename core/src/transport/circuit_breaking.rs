// Copyright 2017-2020 Parity Technologies (UK) Ltd.
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

use crate::{transport::TransportError, Multiaddr, Transport};

use fnv::FnvHashMap;
use futures::prelude::*;
use futures_timer::Delay;
use log::debug;
use multiaddr::Protocol;
use parking_lot::Mutex;
use pin_project::pin_project;
use std::{
    cmp,
    collections::hash_map::Entry,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, Instant},
};

/// Timeout set when opening CircuitBreaker.
const INITIAL_TIMEOUT: Duration = Duration::from_secs(2);
/// Maximum allowed timeout.
const MAX_TIMEOUT: Duration = Duration::from_secs(300);

/// Wraps a [`Transport`] and implements a [circuit breaker
/// pattern](https://docs.microsoft.com/en-us/azure/architecture/patterns/circuit-breaker)
/// for addresses that cannot be dialed. The circuit breaker pattern aims to
/// save resources by delaying an operation which is likely to fail.
///
/// If [`Transport::dial`] fails, a circuit is opened for the [`CircuitAddr`]
/// corresponding to the dialed address. When dialing an address whose
/// `CircuitAddr` has an open circuit, [`CircuitBreakingDial`] adds a delay
/// before calling `dial` on the underlying `Transport`. The delay increases if
/// dialing fails repeatedly.
///
/// To understand why circuits are not opened for `Multiaddr`, see [`CircuitAddr`].
///
/// Open circuits are closed automatically after some time. In addition, an open
/// circuit for an address is closed after successfully dialing that address.
#[derive(Clone)]
pub struct CircuitBreaking<TInner> {
    /// The underlying transport.
    inner: TInner,
    state: Arc<CircuitBreakerState>,
}

/// Represents open circuits.
struct CircuitBreakerState {
    addresses: Mutex<FnvHashMap<CircuitAddr, AddrInfo>>,
}

#[derive(Debug)]
struct AddrInfo {
    /// The delay to add before calling [`Transport::dial`].
    wait_duration: Duration,
    /// After `expiration` the circuit can be closed.
    expiration: Instant,
}

impl CircuitBreakerState {
    fn new() -> Self {
        CircuitBreakerState {
            addresses: Mutex::new(Default::default()),
        }
    }

    /// Returns `None` if circuit of `addr` is closed.
    fn get_wait_duration(&self, addr: &Multiaddr) -> Option<Duration> {
        let circuit_addr = CircuitAddr::from(addr.clone());
        self.addresses
            .lock()
            .get(&circuit_addr)
            .map(|addr_info| addr_info.wait_duration)
    }

    fn open_or_extend_circuit(&self, addr: Multiaddr) {
        let circuit_addr = CircuitAddr::from(addr);
        match self.addresses.lock().entry(circuit_addr) {
            Entry::Vacant(entry) => {
                let inserted = entry.insert(AddrInfo {
                    wait_duration: INITIAL_TIMEOUT,
                    expiration: Instant::now() + INITIAL_TIMEOUT * 2,
                });
                debug!("Opened circuit: {:?}", inserted);
            }

            Entry::Occupied(entry) => {
                let addr = entry.key().clone();
                let addr_info = entry.into_mut();
                addr_info.wait_duration = cmp::min(addr_info.wait_duration * 2, MAX_TIMEOUT);
                addr_info.expiration = Instant::now() + addr_info.wait_duration * 2;
                debug!("Extended circuit {:?}: {:?}", addr, addr_info);
            }
        }
    }

    /// Closes the circuit of `addr` even if the circuit has not yet expired.
    fn close_circuit(&self, addr: &Multiaddr) {
        let circuit_addr = CircuitAddr::from(addr.clone());
        let removed = self.addresses.lock().remove(&circuit_addr);
        debug!("Closed circuit {:?}", removed);
    }

    fn close_expired_circuits(&self) {
        let now = Instant::now();
        let mut addresses = self.addresses.lock();
        addresses.retain(|_, a| a.expiration > now);
        debug!("Closed expired circuits")
    }
}

/// Wraps a `Multiaddr` whose `PeerId` (if any) has been
/// [decapsulated](https://github.com/libp2p/specs/blob/master/addressing/README.md#decapsulation).
///
/// In large public networks (e.g. IPFS) many nodes advertise LAN local
/// addresses. In most cases nodes don't share the same LAN and thus dialing
/// these addresses fails.
///
/// Ignoring the `PeerId` in addresses prevents dialing e.g. `192.168.2.108`
/// multiple times in a row.
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
struct CircuitAddr(Multiaddr);

impl From<Multiaddr> for CircuitAddr {
    fn from(mut addr: Multiaddr) -> Self {
        // Check if `addr` contains `Protocol::P2p(_)`.
        let contained_p2p = addr.iter().find(|protocol| match protocol {
            Protocol::P2p(_) => true,
            _ => false,
        });
        if contained_p2p == None {
            return Self(addr);
        }

        // Decapsulate `Protocol::P2p(_)`.
        loop {
            match addr.pop() {
                Some(Protocol::P2p(_)) | None => break,
                _ => {}
            }
        }
        Self(addr)
    }
}

impl<TInner> CircuitBreaking<TInner> {
    pub fn new(inner: TInner) -> Self {
        CircuitBreaking {
            inner,
            state: Arc::new(CircuitBreakerState::new()),
        }
    }
}

impl<TInner> Transport for CircuitBreaking<TInner>
where
    TInner: Transport,
    TInner::Error: 'static,
{
    type Output = TInner::Output;
    type Error = TInner::Error;
    type Listener = TInner::Listener;
    type ListenerUpgrade = TInner::ListenerUpgrade;
    type Dial = CircuitBreakingDial<TInner>;

    fn listen_on(self, addr: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
        self.inner.listen_on(addr)
    }

    fn dial(self, addr: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
        // Purge expired addresses.
        self.state.close_expired_circuits();

        let inner = match self.state.get_wait_duration(&addr) {
            None => {
                debug!("Dialing address immediately (no open circuit): {:?}", addr);
                let future = match self.inner.dial(addr.clone()) {
                    Ok(f) => Ok(f),
                    Err(TransportError::MultiaddrNotSupported(err)) => {
                        // Avoid open circuits for unsupported addresses.
                        return Err(TransportError::MultiaddrNotSupported(err));
                    }
                    Err(err) => {
                        self.state.open_or_extend_circuit(addr.clone());
                        Err(err)
                    }
                };
                CircuitBreakingDialInner::Dialing(future?)
            }

            Some(wait_duration) => {
                debug!("Wait before dialing address (open circuit): {:?}", addr);
                let timer = Delay::new(wait_duration);
                CircuitBreakingDialInner::Waiting(timer, Some(self.inner))
            }
        };

        Ok(CircuitBreakingDial {
            inner,
            addr,
            state: self.state,
        })
    }

    fn address_translation(&self, listen: &Multiaddr, observed: &Multiaddr) -> Option<Multiaddr> {
        self.inner.address_translation(listen, observed)
    }
}

/// Wraps around a `Future` that produces a connection. Adds a delay before
/// dialing the address if its circuit is opened.
#[pin_project]
pub struct CircuitBreakingDial<TInner>
where
    TInner: Transport,
{
    /// The inner state of the future.
    #[pin]
    inner: CircuitBreakingDialInner<TInner>,
    /// The address to be dialed.
    addr: Multiaddr,
    /// The shared state containing information about circuits.
    state: Arc<CircuitBreakerState>,
}

#[pin_project(project = CircuitBreakingDialInnerProj)]
enum CircuitBreakingDialInner<TInner>
where
    TInner: Transport,
{
    /// Due to an open circuit the address is diales only after `Delay`.
    Waiting(#[pin] Delay, Option<TInner>),
    /// The address is currently being dialed.
    Dialing(#[pin] TInner::Dial),
}

impl<TInner> Future for CircuitBreakingDial<TInner>
where
    TInner: Transport,
{
    type Output = Result<TInner::Output, TInner::Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut this = self.project();

        loop {
            match this.inner.as_mut().project() {
                CircuitBreakingDialInnerProj::Waiting(delay, transport) => {
                    match delay.poll(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(()) => {
                            debug!(
                                "Dialing address after circuit breaking wait expired: {:?}",
                                this.addr
                            );
                            let transport = transport
                                .take()
                                .expect("future called after being finished");
                            match transport.dial(this.addr.clone()) {
                                Ok(f) => this.inner.set(CircuitBreakingDialInner::Dialing(f)),
                                Err(TransportError::MultiaddrNotSupported(_addr)) => {
                                    // TODO do something other than panic?
                                    panic!("Open circuit for unsupported Multiaddr")
                                }
                                Err(TransportError::Other(err)) => {
                                    this.state.open_or_extend_circuit(this.addr.clone());
                                    return Poll::Ready(Err(err));
                                }
                            }
                        }
                    }
                }

                CircuitBreakingDialInnerProj::Dialing(dial) => {
                    match dial.poll(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready(Ok(output)) => {
                            // On success, remove the address.
                            debug!("Successfully dialed addr: {:?}", this.addr);
                            this.state.close_circuit(&this.addr);
                            return Poll::Ready(Ok(output));
                        }
                        Poll::Ready(Err(err)) => {
                            this.state.open_or_extend_circuit(this.addr.clone());
                            return Poll::Ready(Err(err));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::transport::{ListenerEvent, MemoryTransport, TransportError};

    impl CircuitBreakerState {
        pub fn circuit_is_open_for(&self, addr: &Multiaddr) -> bool {
            let circuit_addr = CircuitAddr::from(addr.clone());
            self.addresses.lock().get(&circuit_addr).is_some()
        }
    }

    #[test]
    fn circuit_addr_from_multi_addr() {
        let empty = CircuitAddr::from(Multiaddr::empty());
        assert_eq!(empty.0.to_string(), "");

        let without_p2p = CircuitAddr::from("/ip4/7.7.7.7/tcp/1234".parse::<Multiaddr>().unwrap());
        assert_eq!(without_p2p.0.to_string(), "/ip4/7.7.7.7/tcp/1234");

        let with_p2p = CircuitAddr::from(
            "/ip4/7.7.7.7/tcp/1234/p2p/QmYyQSo1c1Ym7orWxLYvCrM2EmxFTANf8wXmmE7DWjhx5N"
                .parse::<Multiaddr>()
                .unwrap(),
        );
        assert_eq!(with_p2p.0.to_string(), "/ip4/7.7.7.7/tcp/1234");
    }

    /// A [`Transport`] with [`Transport::dial`] always resolving to `Err`.
    #[derive(Debug, Clone)]
    struct DummyTransport;

    impl Transport for DummyTransport {
        type Output = ();
        type Error = std::io::Error;
        type Listener = futures::stream::Pending<
            Result<ListenerEvent<Self::ListenerUpgrade, Self::Error>, Self::Error>,
        >;
        type ListenerUpgrade = futures::future::Pending<Result<Self::Output, std::io::Error>>;
        type Dial = futures::future::Ready<Result<Self::Output, std::io::Error>>;

        fn listen_on(self, _: Multiaddr) -> Result<Self::Listener, TransportError<Self::Error>> {
            panic!()
        }

        fn dial(self, _: Multiaddr) -> Result<Self::Dial, TransportError<Self::Error>> {
            Ok(futures::future::err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "DummyTrans dial error",
            )))
        }

        fn address_translation(&self, _: &Multiaddr, _: &Multiaddr) -> Option<Multiaddr> {
            None
        }
    }

    #[test]
    fn dial_resolving_to_error_opens_circuit() {
        // Test circuit is opened if `Transport::Dial` resolves to `Err(_)`.

        let listen_addr: Multiaddr = "/memory/851372461027149".parse().unwrap();

        futures::executor::block_on(async move {
            let circuit_breaking = CircuitBreaking::new(DummyTransport);
            assert!(circuit_breaking
                .clone()
                .dial(listen_addr.clone())
                .unwrap()
                .await
                .is_err());
            assert!(circuit_breaking.state.circuit_is_open_for(&listen_addr));
        });
    }

    #[test]
    fn dial_returning_error_opens_circuit() {
        // Test circuit is opened if `Transport::dial` returns `Err(_)`.

        let listen_addr: Multiaddr = "/memory/851372461027149001".parse().unwrap();
        let transport = MemoryTransport::default();
        let circuit_breaking = CircuitBreaking::new(transport);
        assert!(circuit_breaking.clone().dial(listen_addr.clone()).is_err());
        assert!(circuit_breaking.state.circuit_is_open_for(&listen_addr));
    }

    #[test]
    fn dialing_with_open_circuit() {
        let listen_addr: Multiaddr = "/memory/851372461027149002".parse().unwrap();
        let transport = MemoryTransport::default();
        let circuit_breaking = CircuitBreaking::new(transport);

        futures::executor::block_on(async move {
            // Dialing unreachable address opens circuit.
            assert!(circuit_breaking.clone().dial(listen_addr.clone()).is_err());
            assert!(circuit_breaking.state.circuit_is_open_for(&listen_addr));

            // Dialing again once address is rechable returns waiting `Dial`.
            let _listener = transport.listen_on(listen_addr.clone()).unwrap();
            let dial = circuit_breaking.clone().dial(listen_addr.clone()).unwrap();
            match dial.inner {
                CircuitBreakingDialInner::Waiting(_, _) => {}
                _ => panic!(),
            }

            // Circuit is closed after dial is resolved with `Ok(_)`.
            dial.await.unwrap();
            assert!(!circuit_breaking.state.circuit_is_open_for(&listen_addr));
        })
    }

    #[test]
    fn dial_repeatedly_failing_extends_delay() {
        let listen_addr: Multiaddr = "/memory/851372461027149003".parse().unwrap();
        let transport = MemoryTransport::default();
        let circuit_breaking = CircuitBreaking::new(transport);

        futures::executor::block_on(async move {
            // Dialing unreachable address opens circuit.
            assert!(circuit_breaking.clone().dial(listen_addr.clone()).is_err());
            let wait_duration_1 = circuit_breaking
                .state
                .get_wait_duration(&listen_addr)
                .unwrap();

            // Consecutive attempt returns a `Dial` resolving to error.
            assert!(circuit_breaking
                .clone()
                .dial(listen_addr.clone())
                .unwrap()
                .await
                .is_err());
            let wait_duration_2 = circuit_breaking
                .state
                .get_wait_duration(&listen_addr)
                .unwrap();
            assert!(wait_duration_2 > wait_duration_1);
        })
    }
}
