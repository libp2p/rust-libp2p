// Copyright 2021 Oliver Wangler <oliver@wngr.de>
// Copyright 2019 Pierre Krieger
// Copyright (c) 2019 Tokio Contributors
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software is furnished to do so,
// subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS
// OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
// WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
// CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// Initial version copied from
// https://github.com/tomaka/wasm-timer/blob/8964804eff980dd3eb115b711c57e481ba541708/src/timer/interval.rs
// and adapted.
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::prelude::*;
use futures_timer::Delay;
use instant::Instant;
use pin_project::pin_project;

/// A stream representing notifications at fixed interval
///
/// Intervals are created through the `Interval::new` or
/// `Interval::new_intial` methods indicating when a first notification
/// should be triggered and when it will be repeated.
///
/// Note that intervals are not intended for high resolution timers, but rather
/// they will likely fire some granularity after the exact instant that they're
/// otherwise indicated to fire at.
#[pin_project]
#[derive(Debug)]
pub struct Interval {
    #[pin]
    delay: Delay,
    interval: Duration,
    fires_at: Instant,
}

impl Interval {
    /// Creates a new interval which will fire at `dur` time into the future,
    /// and will repeat every `dur` interval after
    ///
    /// The returned object will be bound to the default timer for this thread.
    /// The default timer will be spun up in a helper thread on first use.
    pub fn new(dur: Duration) -> Interval {
        Interval::new_initial(dur, dur)
    }

    /// Creates a new interval which will fire the first time after the specified `initial_delay`,
    /// and then will repeat every `dur` interval after.
    ///
    /// The returned object will be bound to the default timer for this thread.
    /// The default timer will be spun up in a helper thread on first use.
    pub fn new_initial(initial_delay: Duration, dur: Duration) -> Interval {
        let fires_at = Instant::now() + initial_delay;
        Interval {
            delay: Delay::new(initial_delay),
            interval: dur,
            fires_at,
        }
    }
}

impl Stream for Interval {
    type Item = ();

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.as_mut().project().delay.poll(cx).is_pending() {
            return Poll::Pending;
        }
        let next = next_interval(self.fires_at, Instant::now(), self.interval);
        self.delay.reset(next);
        self.fires_at += next;
        Poll::Ready(Some(()))
    }
}

/// Converts Duration object to raw nanoseconds if possible
///
/// This is useful to divide intervals.
///
/// While technically for large duration it's impossible to represent any
/// duration as nanoseconds, the largest duration we can represent is about
/// 427_000 years. Large enough for any interval we would use or calculate in
/// tokio.
fn duration_to_nanos(dur: Duration) -> Option<u64> {
    let v = dur.as_secs().checked_mul(1_000_000_000)?;
    v.checked_add(dur.subsec_nanos() as u64)
}

fn next_interval(prev: Instant, now: Instant, interval: Duration) -> Duration {
    let new = prev + interval;
    if new > now {
        interval
    } else {
        let spent_ns =
            duration_to_nanos(now.duration_since(prev)).expect("interval should be expired");
        let interval_ns =
            duration_to_nanos(interval).expect("interval is less that 427 thousand years");
        let mult = spent_ns / interval_ns + 1;
        assert!(
            mult < (1 << 32),
            "can't skip more than 4 billion intervals of {:?} \
             (trying to skip {})",
            interval,
            mult
        );
        interval * mult as u32
    }
}

#[cfg(test)]
mod test {
    use super::next_interval;
    use std::time::{Duration, Instant};

    struct Timeline(Instant);

    impl Timeline {
        fn new() -> Timeline {
            Timeline(Instant::now())
        }
        fn at(&self, millis: u64) -> Instant {
            self.0 + Duration::from_millis(millis)
        }
        fn at_ns(&self, sec: u64, nanos: u32) -> Instant {
            self.0 + Duration::new(sec, nanos)
        }
    }

    fn dur(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    // The math around Instant/Duration isn't 100% precise due to rounding
    // errors
    fn almost_eq(a: Instant, b: Instant) -> bool {
        let diff = match a.cmp(&b) {
            std::cmp::Ordering::Less => b - a,
            std::cmp::Ordering::Equal => return true,
            std::cmp::Ordering::Greater => a - b,
        };

        diff < Duration::from_millis(1)
    }

    #[test]
    fn norm_next() {
        let tm = Timeline::new();
        assert!(almost_eq(
            tm.at(1) + next_interval(tm.at(1), tm.at(2), dur(10)),
            tm.at(11)
        ));
        assert!(almost_eq(
            tm.at(7777) + next_interval(tm.at(7777), tm.at(7788), dur(100)),
            tm.at(7877)
        ));
        assert!(almost_eq(
            tm.at(1) + next_interval(tm.at(1), tm.at(1000), dur(2100)),
            tm.at(2101)
        ));
    }

    #[test]
    fn fast_forward() {
        let tm = Timeline::new();

        assert!(almost_eq(
            tm.at(1) + next_interval(tm.at(1), tm.at(1000), dur(10)),
            tm.at(1001)
        ));
        assert!(almost_eq(
            tm.at(7777) + next_interval(tm.at(7777), tm.at(8888), dur(100)),
            tm.at(8977)
        ));
        assert!(almost_eq(
            tm.at(1) + next_interval(tm.at(1), tm.at(10000), dur(2100)),
            tm.at(10501)
        ));
    }

    /// TODO: this test actually should be successful, but since we can't
    ///       multiply Duration on anything larger than u32 easily we decided
    ///       to allow it to fail for now
    #[test]
    #[should_panic(expected = "can't skip more than 4 billion intervals")]
    fn large_skip() {
        let tm = Timeline::new();
        assert_eq!(
            tm.0 + next_interval(tm.at_ns(0, 1), tm.at_ns(25, 0), Duration::new(0, 2)),
            tm.at_ns(25, 1)
        );
    }
}
