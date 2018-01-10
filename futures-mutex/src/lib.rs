//! A Mutex for the Future(s)
//!
//! API is similar to [`futures::sync::BiLock`](https://docs.rs/futures/0.1.11/futures/sync/struct.BiLock.html)
//! However, it can be cloned into as many handles as desired.
//!

extern crate futures;
extern crate parking_lot;

use std::ops::{Deref, DerefMut};
use std::mem;
use std::sync::Arc;
use futures::task::{current, Task};
use futures::{Future, Poll, Async};
use parking_lot::{Mutex as RegularMutex, MutexGuard as RegularMutexGuard};

#[derive(Debug)]
struct Inner<T> {
    data: RegularMutex<T>,
    wait_queue: RegularMutex<Vec<Task>>,
}

/// A Mutex designed for use inside Futures. Works like `BiLock<T>` from the `futures` crate, but
/// with more than 2 handles.
///
/// **THIS IS NOT A GENRAL PURPOSE MUTEX! IF YOU CALL `poll_lock` OR `lock` OUTSIDE THE CONTEXT OF A TASK, IT WILL PANIC AND EAT YOUR LAUNDRY.**
///
/// This type provides a Mutex that will track tasks that are requesting the Mutex, and will unpark
/// them in the order they request the lock.
///
/// *As of now, there is no strong guarantee that a particular handle of the lock won't be starved. Hopefully the use of the queue will prevent this, but I haven't tried to test that.*
#[derive(Debug)]
pub struct Mutex<T> {
    inner: Arc<Inner<T>>,
}

impl<T> Mutex<T> {
    /// Create a new Mutex wrapping around a value `t`
    pub fn new(t: T) -> Mutex<T> {
        let inner = Arc::new(Inner {
            wait_queue: RegularMutex::new(Vec::new()),
            data: RegularMutex::new(t),
        });

        Mutex {
            inner: inner
        }
    }

    /// This will attempt a non-blocking lock on the mutex, returning `Async::NotReady` if it
    /// can't be acquired.
    ///
    /// This function will return immediatly with a `MutexGuard` if the lock was acquired
    /// successfully. When it drops, the lock will be unlocked.
    ///
    /// If it can't acquire the lock, it will schedule the current task to be unparked when it
    /// might be able lock the mutex again.
    ///
    /// # Panics
    ///
    /// This function will panic if called outside the context of a future's task.
    pub fn poll_lock(&self) -> Async<MutexGuard<T>> {
        let mut ext_lock = self.inner.wait_queue.lock();
        match self.inner.data.try_lock() {
            Some(guard) => {
                Async::Ready(MutexGuard {
                    inner: &self.inner,
                    guard: Some(guard),
                })
            },
            None => {
                ext_lock.push(current());
                Async::NotReady
            },
        }
    }

    /// Convert this lock into a future that resolves to a guard that allows access to the data.
    /// This function returns `MutexAcquire<T>`, which resolves to a `MutexGuard<T>`
    /// guard type.
    ///
    /// The returned future will never return an error.
    pub fn lock(&self) -> MutexAcquire<T> {
        MutexAcquire {
            inner: self
        }
    }
}

impl<T> Clone for Mutex<T> {
    fn clone(&self) -> Mutex<T> {
        Mutex {
            inner: self.inner.clone()
        }
    }
}

/// Returned RAII guard from the `poll_lock` method.
///
/// This structure acts as a sentinel to the data in the `Mutex<T>` itself,
/// implementing `Deref` and `DerefMut` to `T`. When dropped, the lock will be
/// unlocked.
// TODO: implement Debug
pub struct MutexGuard<'a, T: 'a> {
    inner: &'a Inner<T>,
    guard: Option<RegularMutexGuard<'a, T>>,
}

impl<'a, T> Deref for MutexGuard<'a, T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.guard.as_ref().expect("mutex wasn't locked").deref()
    }
}

impl<'a, T> DerefMut for MutexGuard<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        self.guard.as_mut().expect("mutex wasn't locked").deref_mut()
    }
}

impl<'a, T> Drop for MutexGuard<'a, T> {
    fn drop(&mut self) {
        let mut wait_queue_lock = self.inner.wait_queue.lock();
        let _ = self.guard.take().expect("mutex was unlocked");
        for task in wait_queue_lock.drain(..) {
            task.notify();
        }
    }
}

/// Future returned by `Mutex::lock` which resolves to a guard when a lock is acquired.
#[derive(Debug)]
pub struct MutexAcquire<'a, T: 'a> {
    inner: &'a Mutex<T>
}

impl<'a, T> Future for MutexAcquire<'a, T> {
    type Item = MutexGuard<'a, T>;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        Ok(self.inner.poll_lock())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::{self, Notify};
    use futures::future;
    use futures::stream::{self, Stream};
    use std::thread;

    struct Foo;

    impl Notify for Foo {
        fn notify(&self, id: usize) {}
    }

    #[test]
    fn simple() {
        let future = future::lazy(|| {
            let lock1 = Mutex::new(1);
            let lock2 = lock1.clone();
            let lock3 = lock1.clone();

            let mut guard = match lock1.poll_lock() {
                Async::Ready(g) => g,
                Async::NotReady => panic!("poll not ready"),
            };
            assert_eq!(*guard, 1);
            *guard = 2;

            assert!(lock1.poll_lock().is_not_ready());
            assert!(lock2.poll_lock().is_not_ready());
            assert!(lock3.poll_lock().is_not_ready());
            drop(guard);

            assert!(lock1.poll_lock().is_ready());
            assert!(lock2.poll_lock().is_ready());
            assert!(lock3.poll_lock().is_ready());

            let guard = match lock2.poll_lock() {
                Async::Ready(g) => g,
                Async::NotReady => panic!("poll not ready"),
            };
            assert_eq!(*guard, 2);

            assert!(lock1.poll_lock().is_not_ready());
            assert!(lock2.poll_lock().is_not_ready());
            assert!(lock3.poll_lock().is_not_ready());

            Ok::<(), ()>(())
        });

        assert!(executor::spawn(future)
                .poll_future_notify(&Arc::new(Foo), 0)
                .expect("failure in poll")
                .is_ready());
    }

    #[test]
    fn concurrent() {
        const N: usize = 10000;
        let lock1 = Mutex::new(0);
        let lock2 = lock1.clone();

        let a = Increment {
            a: Some(lock1),
            remaining: N,
        };
        let b = a.clone();

        let t1 = thread::spawn(move || a.wait());
        let b = b.wait().expect("b error");
        let a = t1.join().unwrap().expect("a error");

        match a.poll_lock() {
            Async::Ready(l) => assert_eq!(*l, 2 * N),
            Async::NotReady => panic!("poll not ready"),
        }
        match b.poll_lock() {
            Async::Ready(l) => assert_eq!(*l, 2 * N),
            Async::NotReady => panic!("poll not ready"),
        }

        #[derive(Clone)]
        struct Increment {
            remaining: usize,
            a: Option<Mutex<usize>>,
        }

        impl Future for Increment {
            type Item = Mutex<usize>;
            type Error = ();

            fn poll(&mut self) -> Poll<Mutex<usize>, ()> {
                loop {
                    if self.remaining == 0 {
                        return Ok(self.a.take().unwrap().into())
                    }

                    let a = self.a.as_ref().unwrap();
                    let mut a = match a.poll_lock() {
                        Async::Ready(l) => l,
                        Async::NotReady => return Ok(Async::NotReady),
                    };
                    self.remaining -= 1;
                    *a += 1;
                }
            }
        }
    }
}
