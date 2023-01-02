// Copyright 2021 Protocol Labs.
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

pub use generic::{
    RateLimiter as GenericRateLimiter, RateLimiterConfig as GenericRateLimiterConfig,
};
use instant::Instant;
use libp2p_core::multiaddr::{Multiaddr, Protocol};
use libp2p_core::PeerId;
use std::net::IpAddr;

/// Allows rate limiting access to some resource based on the [`PeerId`] and
/// [`Multiaddr`] of a remote peer.
///
/// See [`new_per_peer`] and [`new_per_ip`] for precast implementations. Use
/// [`GenericRateLimiter`] to build your own, e.g. based on the autonomous system
/// number of a peers IP address.
pub trait RateLimiter: Send {
    fn try_next(&mut self, peer: PeerId, addr: &Multiaddr, now: Instant) -> bool;
}

pub fn new_per_peer(config: GenericRateLimiterConfig) -> Box<dyn RateLimiter> {
    let mut limiter = GenericRateLimiter::new(config);
    Box::new(move |peer_id, _addr: &Multiaddr, now| limiter.try_next(peer_id, now))
}

pub fn new_per_ip(config: GenericRateLimiterConfig) -> Box<dyn RateLimiter> {
    let mut limiter = GenericRateLimiter::new(config);
    Box::new(move |_peer_id, addr: &Multiaddr, now| {
        multiaddr_to_ip(addr)
            .map(|a| limiter.try_next(a, now))
            .unwrap_or(true)
    })
}

impl<T: FnMut(PeerId, &Multiaddr, Instant) -> bool + Send> RateLimiter for T {
    fn try_next(&mut self, peer: PeerId, addr: &Multiaddr, now: Instant) -> bool {
        self(peer, addr, now)
    }
}

fn multiaddr_to_ip(addr: &Multiaddr) -> Option<IpAddr> {
    addr.iter().find_map(|p| match p {
        Protocol::Ip4(addr) => Some(addr.into()),
        Protocol::Ip6(addr) => Some(addr.into()),
        _ => None,
    })
}

mod generic {
    use instant::Instant;
    use std::collections::{HashMap, VecDeque};
    use std::convert::TryInto;
    use std::hash::Hash;
    use std::num::NonZeroU32;
    use std::time::Duration;

    /// Rate limiter using the [Token Bucket] algorithm.
    ///
    /// [Token Bucket]: https://en.wikipedia.org/wiki/Token_bucket
    pub struct RateLimiter<Id> {
        limit: u32,
        interval: Duration,

        refill_schedule: VecDeque<(Instant, Id)>,
        buckets: HashMap<Id, u32>,
    }

    /// Configuration for a [`RateLimiter`].
    #[derive(Debug, Clone, Copy)]
    pub struct RateLimiterConfig {
        /// The maximum number of tokens in the bucket at any point in time.
        pub limit: NonZeroU32,
        /// The interval at which a single token is added to the bucket.
        pub interval: Duration,
    }

    impl<Id: Eq + PartialEq + Hash + Clone> RateLimiter<Id> {
        pub(crate) fn new(config: RateLimiterConfig) -> Self {
            assert!(!config.interval.is_zero());

            Self {
                limit: config.limit.into(),
                interval: config.interval,
                refill_schedule: Default::default(),
                buckets: Default::default(),
            }
        }

        pub(crate) fn try_next(&mut self, id: Id, now: Instant) -> bool {
            self.refill(now);

            match self.buckets.get_mut(&id) {
                // If the bucket exists, try to take a token.
                Some(balance) => match balance.checked_sub(1) {
                    Some(a) => {
                        *balance = a;
                        true
                    }
                    None => false,
                },
                // If the bucket is missing, act like the bucket has `limit` number of tokens. Take one
                // token and track the new bucket balance.
                None => {
                    self.buckets.insert(id.clone(), self.limit - 1);
                    self.refill_schedule.push_back((now, id));
                    true
                }
            }
        }

        fn refill(&mut self, now: Instant) {
            // Note when used with a high number of buckets: This loop refills all the to-be-refilled
            // buckets at once, thus potentially delaying the parent call to `try_next`.
            loop {
                match self.refill_schedule.get(0) {
                    // Only continue if (a) there is a bucket and (b) the bucket has not already been
                    // refilled recently.
                    Some((last_refill, _)) if now.duration_since(*last_refill) >= self.interval => {
                    }
                    // Otherwise stop refilling. Items in `refill_schedule` are sorted, thus, if the
                    // first ain't ready, none of them are.
                    _ => return,
                };

                let (last_refill, id) = self
                    .refill_schedule
                    .pop_front()
                    .expect("Queue not to be empty.");

                // Get the current balance of the bucket.
                let balance = self
                    .buckets
                    .get(&id)
                    .expect("Entry can only be removed via refill.");

                // Calculate the new balance.
                let duration_since = now.duration_since(last_refill);
                let new_tokens = duration_since
                    .as_micros()
                    // Note that the use of `as_micros` limits the number of tokens to 10^6 per second.
                    .checked_div(self.interval.as_micros())
                    .and_then(|i| i.try_into().ok())
                    .unwrap_or(u32::MAX);
                let new_balance = balance.checked_add(new_tokens).unwrap_or(u32::MAX);

                // If the new balance is below the limit, update the bucket.
                if new_balance < self.limit {
                    self.buckets
                        .insert(id.clone(), new_balance)
                        .expect("To override value.");
                    self.refill_schedule.push_back((now, id));
                } else {
                    // If the balance is above the limit, the bucket can be removed, given that a
                    // non-existing bucket is equivalent to a bucket with `limit` tokens.
                    self.buckets.remove(&id);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use quickcheck::{QuickCheck, TestResult};
        use std::num::NonZeroU32;

        #[test]
        fn first() {
            let id = 1;
            let mut l = RateLimiter::new(RateLimiterConfig {
                limit: NonZeroU32::new(10).unwrap(),
                interval: Duration::from_secs(1),
            });
            assert!(l.try_next(id, Instant::now()));
        }

        #[test]
        fn limits() {
            let id = 1;
            let now = Instant::now();
            let mut l = RateLimiter::new(RateLimiterConfig {
                limit: NonZeroU32::new(10).unwrap(),
                interval: Duration::from_secs(1),
            });
            for _ in 0..10 {
                assert!(l.try_next(id, now));
            }

            assert!(!l.try_next(id, now));
        }

        #[test]
        fn refills() {
            let id = 1;
            let now = Instant::now();
            let mut l = RateLimiter::new(RateLimiterConfig {
                limit: NonZeroU32::new(10).unwrap(),
                interval: Duration::from_secs(1),
            });

            for _ in 0..10 {
                assert!(l.try_next(id, now));
            }
            assert!(!l.try_next(id, now));

            let now = now + Duration::from_secs(1);
            assert!(l.try_next(id, now));
            assert!(!l.try_next(id, now));

            let now = now + Duration::from_secs(10);
            for _ in 0..10 {
                assert!(l.try_next(id, now));
            }
        }

        #[test]
        fn move_at_half_interval_steps() {
            let id = 1;
            let now = Instant::now();
            let mut l = RateLimiter::new(RateLimiterConfig {
                limit: NonZeroU32::new(1).unwrap(),
                interval: Duration::from_secs(2),
            });

            assert!(l.try_next(id, now));
            assert!(!l.try_next(id, now));

            let now = now + Duration::from_secs(1);
            assert!(!l.try_next(id, now));

            let now = now + Duration::from_secs(1);
            assert!(l.try_next(id, now));
        }

        #[test]
        fn garbage_collects() {
            let now = Instant::now();
            let mut l = RateLimiter::new(RateLimiterConfig {
                limit: NonZeroU32::new(1).unwrap(),
                interval: Duration::from_secs(1),
            });

            assert!(l.try_next(1, now));

            let now = now + Duration::from_secs(1);
            assert!(l.try_next(2, now));

            assert_eq!(l.buckets.len(), 1);
            assert_eq!(l.refill_schedule.len(), 1);
        }

        #[test]
        fn quick_check() {
            fn prop(
                limit: NonZeroU32,
                interval: Duration,
                events: Vec<(u32, Duration)>,
            ) -> TestResult {
                if interval.is_zero() {
                    return TestResult::discard();
                }

                let mut now = Instant::now();
                let mut l = RateLimiter::new(RateLimiterConfig { limit, interval });

                for (id, d) in events {
                    now = if let Some(now) = now.checked_add(d) {
                        now
                    } else {
                        return TestResult::discard();
                    };
                    l.try_next(id, now);
                }

                now = if let Some(now) = interval
                    .checked_mul(limit.into())
                    .and_then(|full_interval| now.checked_add(full_interval))
                {
                    now
                } else {
                    return TestResult::discard();
                };
                assert!(l.try_next(1, now));

                assert_eq!(l.buckets.len(), 1);
                assert_eq!(l.refill_schedule.len(), 1);

                TestResult::passed()
            }

            QuickCheck::new().quickcheck(prop as fn(_, _, _) -> _)
        }
    }
}
