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

use std::{
    marker::Unpin,
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, Instant},
};

/// Simple wrapper for the differents type of timers
#[derive(Debug)]
pub struct WrapTimer<T> {
    timer: T,
}

/// Builder interface to homogenize the differents implementations
pub trait TimerBuilder: Send + Unpin + 'static {
    /// Creates a timer that emits an event once at the given time instant.
    fn at(instant: Instant) -> Self;

    /// Creates a timer that emits events periodically.
    fn interval(duration: Duration) -> Self;

    /// Creates a timer that emits events periodically, starting at start.
    fn interval_at(start: Instant, duration: Duration) -> Self;
}

#[cfg(feature = "async-io")]
pub mod asio {
    use super::*;
    use async_io::Timer;
    use futures::Stream;

    /// Async Timer
    pub type AsyncTimer = WrapTimer<Timer>;

    impl TimerBuilder for WrapTimer<Timer> {
        /// Creates a timer that emits an event once at the given time instant.
        fn at(instant: Instant) -> Self {
            WrapTimer {
                timer: Timer::at(instant),
            }
        }

        /// Creates a timer that emits events periodically.
        fn interval(duration: Duration) -> Self {
            WrapTimer {
                timer: Timer::interval(duration),
            }
        }

        /// Creates a timer that emits events periodically, starting at start.
        fn interval_at(start: Instant, duration: Duration) -> Self {
            WrapTimer {
                timer: Timer::interval_at(start, duration),
            }
        }
    }

    impl Stream for AsyncTimer {
        type Item = Instant;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Instant>> {
            Pin::new(&mut self.timer).poll_next(cx)
        }
    }
}

#[cfg(feature = "tokio")]
pub mod tokio {
    use super::*;
    use ::tokio::time::{self, Instant as TokioInstant, Interval};
    use futures::Stream;

    /// Tokio wrapper
    pub type TokioTimer = WrapTimer<Interval>;

    impl TimerBuilder for WrapTimer<Interval> {
        /// Creates a timer that emits an event once at the given time instant.
        fn at(instant: Instant) -> Self {
            // Taken from: https://docs.rs/async-io/1.7.0/src/async_io/lib.rs.html#91
            let timer = time::interval_at(
                TokioInstant::from_std(instant),
                Duration::new(std::u64::MAX, 1_000_000_000 - 1),
            );
            WrapTimer { timer }
        }

        /// Creates a timer that emits events periodically.
        fn interval(duration: Duration) -> Self {
            WrapTimer {
                timer: time::interval(duration),
            }
        }

        /// Creates a timer that emits events periodically, starting at start.
        fn interval_at(start: Instant, duration: Duration) -> Self {
            WrapTimer {
                timer: time::interval_at(TokioInstant::from_std(start), duration),
            }
        }
    }

    impl Stream for TokioTimer {
        type Item = TokioInstant;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.timer.poll_tick(cx).map(Some)
        }
    }
}
