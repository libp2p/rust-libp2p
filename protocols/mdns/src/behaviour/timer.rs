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

use std::task::{Context, Poll};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct WrapTimer<T> {
    timer: T,
}

pub trait TimerBuilder {
    type Item;

    fn at(instant: Instant) -> Self;

    fn interval(duration: Duration) -> Self;

    fn interval_at(start: Instant, duration: Duration) -> Self;

    fn poll_tick(&mut self, cx: &mut Context) -> Poll<Self::Item>;
}

#[cfg(feature = "async-io")]
pub(crate) mod time {
    use super::*;
    use async_io::Timer;
    use futures::Stream;
    use std::pin::Pin;

    pub(crate) type AsyncTimer = WrapTimer<Timer>;

    impl TimerBuilder for WrapTimer<Timer> {
        type Item = Option<Instant>;

        fn at(instant: Instant) -> Self {
            WrapTimer {
                timer: Timer::at(instant),
            }
        }

        fn interval(duration: Duration) -> Self {
            WrapTimer {
                timer: Timer::interval(duration),
            }
        }

        fn interval_at(start: Instant, duration: Duration) -> Self {
            WrapTimer {
                timer: Timer::interval_at(start, duration),
            }
        }

        fn poll_tick(&mut self, cx: &mut Context) -> Poll<Self::Item> {
            Pin::new(&mut self.timer).poll_next(cx)
        }
    }
}

#[cfg(feature = "tokio")]
pub(crate) mod time {
    use super::*;
    use tokio::time::{self, Instant as TokioInstant, Interval};

    pub(crate) type AsyncTimer = WrapTimer<Interval>;

    impl TimerBuilder for WrapTimer<Interval> {
        type Item = time::Instant;

        fn at(instant: Instant) -> Self {
            let timer = time::interval_at(
                TokioInstant::from_std(instant),
                Duration::new(std::u64::MAX, 1_000_000_000 - 1),
            );
            WrapTimer { timer }
        }

        fn interval(duration: Duration) -> Self {
            WrapTimer {
                timer: time::interval(duration),
            }
        }

        fn interval_at(start: Instant, duration: Duration) -> Self {
            WrapTimer {
                timer: time::interval_at(TokioInstant::from_std(start), duration),
            }
        }

        fn poll_tick(&mut self, cx: &mut Context) -> Poll<Self::Item> {
            self.timer.poll_tick(cx)
        }
    }
}
