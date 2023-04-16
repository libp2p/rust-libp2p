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
    time::{Duration, Instant},
};

/// Simple wrapper for the different type of timers
#[derive(Debug)]
#[cfg(any(feature = "async-io", feature = "tokio"))]
pub struct Timer<T> {
    inner: T,
}

/// Builder interface to homogenize the different implementations
#[allow(unreachable_pub)] // Users should not depend on this.
pub trait Builder: Send + Unpin + 'static {
    /// Creates a timer that emits an event once at the given time instant.
    fn at(instant: Instant) -> Self;

    /// Creates a timer that emits events periodically.
    fn interval(duration: Duration) -> Self;

    /// Creates a timer that emits events periodically, starting at start.
    fn interval_at(start: Instant, duration: Duration) -> Self;
}

#[cfg(feature = "async-io")]
pub(crate) mod asio {
    use super::*;
    use async_io::Timer as AsioTimer;
    use futures::Stream;
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };

    /// Async Timer
    pub(crate) type AsyncTimer = Timer<AsioTimer>;
    impl Builder for AsyncTimer {
        fn at(instant: Instant) -> Self {
            Self {
                inner: AsioTimer::at(instant),
            }
        }

        fn interval(duration: Duration) -> Self {
            Self {
                inner: AsioTimer::interval(duration),
            }
        }

        fn interval_at(start: Instant, duration: Duration) -> Self {
            Self {
                inner: AsioTimer::interval_at(start, duration),
            }
        }
    }

    impl Stream for AsyncTimer {
        type Item = Instant;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Pin::new(&mut self.inner).poll_next(cx)
        }
    }
}

#[cfg(feature = "tokio")]
pub(crate) mod tokio {
    use super::*;
    use ::tokio::time::{self, Instant as TokioInstant, Interval, MissedTickBehavior};
    use futures::Stream;
    use std::{
        pin::Pin,
        task::{Context, Poll},
    };

    /// Tokio wrapper
    pub(crate) type TokioTimer = Timer<Interval>;
    impl Builder for TokioTimer {
        fn at(instant: Instant) -> Self {
            // Taken from: https://docs.rs/async-io/1.7.0/src/async_io/lib.rs.html#91
            let mut inner = time::interval_at(
                TokioInstant::from_std(instant),
                Duration::new(std::u64::MAX, 1_000_000_000 - 1),
            );
            inner.set_missed_tick_behavior(MissedTickBehavior::Skip);
            Self { inner }
        }

        fn interval(duration: Duration) -> Self {
            let mut inner = time::interval_at(TokioInstant::now() + duration, duration);
            inner.set_missed_tick_behavior(MissedTickBehavior::Skip);
            Self { inner }
        }

        fn interval_at(start: Instant, duration: Duration) -> Self {
            let mut inner = time::interval_at(TokioInstant::from_std(start), duration);
            inner.set_missed_tick_behavior(MissedTickBehavior::Skip);
            Self { inner }
        }
    }

    impl Stream for TokioTimer {
        type Item = TokioInstant;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            self.inner.poll_tick(cx).map(Some)
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            (std::usize::MAX, None)
        }
    }
}
