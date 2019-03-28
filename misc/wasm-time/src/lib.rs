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

use futures::{prelude::*, sync::oneshot, try_ready};
use std::{error, fmt};
use std::cmp::{Eq, PartialEq, Ord, PartialOrd, Ordering};
use std::ops::{Add, Sub};
use std::time::Duration;
#[cfg(target_os = "unknown")]
use wasm_bindgen::{prelude::*, JsCast};

#[cfg(not(target_os = "unknown"))]
pub type Instant = std::time::Instant;
#[cfg(not(target_os = "unknown"))]
pub type Delay = tokio_timer::Delay;
#[cfg(not(target_os = "unknown"))]
pub type Interval = tokio_timer::Interval;
#[cfg(not(target_os = "unknown"))]
pub type Error = tokio_timer::Error;

#[cfg(target_os = "unknown")]
pub use self::timeout::Timeout;

#[cfg(target_os = "unknown")]
#[derive(Debug, Copy, Clone)]
pub struct Instant {
    inner: f64,
}

#[cfg(target_os = "unknown")]
impl PartialEq for Instant {
    fn eq(&self, other: &Instant) -> bool {
        // TODO: meh
        self.inner == other.inner
    }
}

#[cfg(target_os = "unknown")]
impl Eq for Instant {}

#[cfg(target_os = "unknown")]
impl PartialOrd for Instant {
    fn partial_cmp(&self, other: &Instant) -> Option<Ordering> {
        self.inner.partial_cmp(&other.inner)
    }
}

#[cfg(target_os = "unknown")]
impl Ord for Instant {
    fn cmp(&self, other: &Self) -> Ordering {
        self.inner.partial_cmp(&other.inner).unwrap()
    }
}

#[cfg(target_os = "unknown")]
impl Instant {
    pub fn now() -> Instant {
        let val = web_sys::window()
            .expect("not in a browser")
            .performance()
            .expect("performance object not available")
            .now();
        Instant { inner: val }
    }

    pub fn duration_since(&self, earlier: Instant) -> Duration {
        *self - earlier
    }

    pub fn elapsed(&self) -> Duration {
        Instant::now() - *self
    }
}

#[cfg(target_os = "unknown")]
impl Add<Duration> for Instant {
    type Output = Instant;

    fn add(self, other: Duration) -> Instant {
        let new_val = self.inner + other.as_millis() as f64;
        Instant { inner: new_val as f64 }
    }
}

#[cfg(target_os = "unknown")]
impl Sub<Duration> for Instant {
    type Output = Instant;

    fn sub(self, other: Duration) -> Instant {
        let new_val = self.inner - other.as_millis() as f64;
        Instant { inner: new_val as f64 }
    }
}

#[cfg(target_os = "unknown")]
impl Sub<Instant> for Instant {
    type Output = Duration;

    fn sub(self, other: Instant) -> Duration {
        let ms = self.inner - other.inner;
        assert!(ms >= 0.0);
        Duration::from_millis(ms as u64)
    }
}

#[cfg(target_os = "unknown")]
#[derive(Debug)]
pub struct Error;

impl error::Error for Error {
    fn description(&self) -> &str {
        ""
    }
}

impl fmt::Display for Error {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "")
    }
}

#[cfg(target_os = "unknown")]
pub struct Delay {
    handle: i32,
    deadline: Instant,
    triggered_rx: oneshot::Receiver<()>,
    cb: Closure<FnMut()>,
}

#[cfg(target_os = "unknown")]
impl Delay {
    pub fn new(deadline: Instant) -> Delay {
        let now = Instant::now();
        if deadline > now {
            let dur = deadline - now;
            Delay::new_timeout(deadline, dur)
        } else {
            Delay::new_timeout(deadline, Duration::new(0, 0))
        }
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    fn new_timeout(deadline: Instant, duration: Duration) -> Delay {
        let (tx, rx) = oneshot::channel();
        let mut tx = Some(tx);

        let cb = Closure::wrap(Box::new(move || {
            let _ = tx.take().unwrap().send(());
        }) as Box<FnMut()>);

        let handle = web_sys::window()
            .expect("not in a browser")
            .set_timeout_with_callback_and_timeout_and_arguments_0(cb.as_ref().unchecked_ref(), duration.as_millis() as i32)
            .expect("failed to call set_timeout");

        Delay { handle, triggered_rx: rx, deadline, cb }
    }

    fn reset_timeout(&mut self) {
        // TODO: what does that do?
    }

    pub fn reset(&mut self, deadline: Instant) {
        *self = Delay::new(deadline);
    }
}

#[cfg(target_os = "unknown")]
impl Drop for Delay {
    fn drop(&mut self) {
        web_sys::window().unwrap().clear_timeout_with_handle(self.handle);
    }
}

#[cfg(target_os = "unknown")]
unsafe impl Send for Delay {}       // TODO:
#[cfg(target_os = "unknown")]
unsafe impl Sync for Delay {}       // TODO:

#[cfg(target_os = "unknown")]
impl fmt::Debug for Delay {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_tuple("Delay").field(&self.deadline).finish()
    }
}

#[cfg(target_os = "unknown")]
impl Future for Delay {
    type Item = ();
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        self.triggered_rx.poll().map_err(|_| unreachable!())
    }
}

/// A stream representing notifications at fixed interval
#[derive(Debug)]
pub struct Interval {
    /// Future that completes the next time the `Interval` yields a value.
    delay: Delay,

    /// The duration between values yielded by `Interval`.
    duration: Duration,
}

impl Interval {
    /// Create a new `Interval` that starts at `at` and yields every `duration`
    /// interval after that.
    ///
    /// Note that when it starts, it produces item too.
    ///
    /// The `duration` argument must be a non-zero duration.
    ///
    /// # Panics
    ///
    /// This function panics if `duration` is zero.
    pub fn new(at: Instant, duration: Duration) -> Interval {
        assert!(
            duration > Duration::new(0, 0),
            "`duration` must be non-zero."
        );

        Interval::new_with_delay(Delay::new(at), duration)
    }

    /// Creates new `Interval` that yields with interval of `duration`.
    ///
    /// The function is shortcut for `Interval::new(Instant::now() + duration, duration)`.
    ///
    /// The `duration` argument must be a non-zero duration.
    ///
    /// # Panics
    ///
    /// This function panics if `duration` is zero.
    pub fn new_interval(duration: Duration) -> Interval {
        Interval::new(Instant::now() + duration, duration)
    }

    pub(crate) fn new_with_delay(delay: Delay, duration: Duration) -> Interval {
        Interval { delay, duration }
    }
}

impl Stream for Interval {
    type Item = Instant;
    type Error = crate::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        // Wait for the delay to be done
        let _ = try_ready!(self.delay.poll());

        // Get the `now` by looking at the `delay` deadline
        let now = self.delay.deadline();

        // The next interval value is `duration` after the one that just
        // yielded.
        self.delay.reset(now + self.duration);

        // Return the current instant
        Ok(Some(now).into())
    }
}

#[cfg(target_os = "unknown")]
pub mod timeout {
    use super::{Delay, Instant};
    use futures::prelude::*;
    use std::{error, fmt, time::Duration};

    #[cfg(target_os = "unknown")]
    #[must_use = "futures do nothing unless polled"]
    #[derive(Debug)]
    pub struct Timeout<T> {
        value: T,
        delay: Delay,
    }

    /// Error returned by `Timeout`.
    #[cfg(target_os = "unknown")]
    #[derive(Debug)]
    pub struct Error<T>(Kind<T>);

    /// Timeout error variants
    #[cfg(target_os = "unknown")]
    #[derive(Debug)]
    enum Kind<T> {
        /// Inner value returned an error
        Inner(T),

        /// The timeout elapsed.
        Elapsed,

        /// Timer returned an error.
        Timer(crate::Error),
    }

    #[cfg(target_os = "unknown")]
    impl<T> Timeout<T> {
        /// Create a new `Timeout` that allows `value` to execute for a duration of
        /// at most `timeout`.
        ///
        /// The exact behavior depends on if `value` is a `Future` or a `Stream`.
        ///
        /// See [type] level documentation for more details.
        ///
        /// [type]: #
        ///
        /// # Examples
        ///
        /// Create a new `Timeout` set to expire in 10 milliseconds.
        ///
        /// ```rust
        /// # extern crate futures;
        /// # extern crate tokio;
        /// use tokio::timer::Timeout;
        /// use futures::Future;
        /// use futures::sync::oneshot;
        /// use std::time::Duration;
        ///
        /// # fn main() {
        /// let (tx, rx) = oneshot::channel();
        /// # tx.send(()).unwrap();
        ///
        /// # tokio::runtime::current_thread::block_on_all(
        /// // Wrap the future with a `Timeout` set to expire in 10 milliseconds.
        /// Timeout::new(rx, Duration::from_millis(10))
        /// # ).unwrap();
        /// # }
        /// ```
        pub fn new(value: T, timeout: Duration) -> Timeout<T> {
            let delay = Delay::new_timeout(Instant::now() + timeout, timeout);

            Timeout {
                value,
                delay,
            }
        }

        /// Gets a reference to the underlying value in this timeout.
        pub fn get_ref(&self) -> &T {
            &self.value
        }

        /// Gets a mutable reference to the underlying value in this timeout.
        pub fn get_mut(&mut self) -> &mut T {
            &mut self.value
        }

        /// Consumes this timeout, returning the underlying value.
        pub fn into_inner(self) -> T {
            self.value
        }
    }

    #[cfg(target_os = "unknown")]
    impl<T: Future> Timeout<T> {
        /// Create a new `Timeout` that completes when `future` completes or when
        /// `deadline` is reached.
        ///
        /// This function differs from `new` in that:
        ///
        /// * It only accepts `Future` arguments.
        /// * It sets an explicit `Instant` at which the timeout expires.
        pub fn new_at(future: T, deadline: Instant) -> Timeout<T> {
            let delay = Delay::new(deadline);

            Timeout {
                value: future,
                delay,
            }
        }
    }

    #[cfg(target_os = "unknown")]
    impl<T> Future for Timeout<T>
    where T: Future,
    {
        type Item = T::Item;
        type Error = Error<T::Error>;

        fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
            // First, try polling the future
            match self.value.poll() {
                Ok(Async::Ready(v)) => return Ok(Async::Ready(v)),
                Ok(Async::NotReady) => {}
                Err(e) => return Err(Error::inner(e)),
            }

            // Now check the timer
            match self.delay.poll() {
                Ok(Async::NotReady) => Ok(Async::NotReady),
                Ok(Async::Ready(_)) => {
                    Err(Error::elapsed())
                },
                Err(e) => Err(Error::timer(e)),
            }
        }
    }

    #[cfg(target_os = "unknown")]
    impl<T> Stream for Timeout<T>
    where T: Stream,
    {
        type Item = T::Item;
        type Error = Error<T::Error>;

        fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
            // First, try polling the future
            match self.value.poll() {
                Ok(Async::Ready(v)) => {
                    if v.is_some() {
                        self.delay.reset_timeout();
                    }
                    return Ok(Async::Ready(v))
                }
                Ok(Async::NotReady) => {}
                Err(e) => return Err(Error::inner(e)),
            }

            // Now check the timer
            match self.delay.poll() {
                Ok(Async::NotReady) => Ok(Async::NotReady),
                Ok(Async::Ready(_)) => {
                    self.delay.reset_timeout();
                    Err(Error::elapsed())
                },
                Err(e) => Err(Error::timer(e)),
            }
        }
    }

    impl<T> Error<T> {
        /// Create a new `Error` representing the inner value completing with `Err`.
        pub fn inner(err: T) -> Error<T> {
            Error(Kind::Inner(err))
        }

        /// Returns `true` if the error was caused by the inner value completing
        /// with `Err`.
        pub fn is_inner(&self) -> bool {
            match self.0 {
                Kind::Inner(_) => true,
                _ => false,
            }
        }

        /// Consumes `self`, returning the inner future error.
        pub fn into_inner(self) -> Option<T> {
            match self.0 {
                Kind::Inner(err) => Some(err),
                _ => None,
            }
        }

        /// Create a new `Error` representing the inner value not completing before
        /// the deadline is reached.
        pub fn elapsed() -> Error<T> {
            Error(Kind::Elapsed)
        }

        /// Returns `true` if the error was caused by the inner value not completing
        /// before the deadline is reached.
        pub fn is_elapsed(&self) -> bool {
            match self.0 {
                Kind::Elapsed => true,
                _ => false,
            }
        }

        /// Creates a new `Error` representing an error encountered by the timer
        /// implementation
        pub fn timer(err: crate::Error) -> Error<T> {
            Error(Kind::Timer(err))
        }

        /// Returns `true` if the error was caused by the timer.
        pub fn is_timer(&self) -> bool {
            match self.0 {
                Kind::Timer(_) => true,
                _ => false,
            }
        }

        /// Consumes `self`, returning the error raised by the timer implementation.
        pub fn into_timer(self) -> Option<crate::Error> {
            match self.0 {
                Kind::Timer(err) => Some(err),
                _ => None,
            }
        }
    }

    impl<T: error::Error> error::Error for Error<T> {
        fn description(&self) -> &str {
            use self::Kind::*;

            match self.0 {
                Inner(ref e) => e.description(),
                Elapsed => "deadline has elapsed",
                Timer(ref e) => e.description(),
            }
        }
    }

    impl<T: fmt::Display> fmt::Display for Error<T> {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            use self::Kind::*;

            match self.0 {
                Inner(ref e) => e.fmt(fmt),
                Elapsed => "deadline has elapsed".fmt(fmt),
                Timer(ref e) => e.fmt(fmt),
            }
        }
    }
}
