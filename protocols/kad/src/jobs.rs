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

use crate::record::{Record, ProviderRecord, store::RecordStore};

use libp2p_core::PeerId;
use futures::prelude::*;
use multihash::Multihash;
use std::collections::HashSet;
use std::time::Duration;
use std::vec;
use wasm_timer::{Instant, Delay};

/// The maximum number of queries towards which background jobs
/// are allowed to start new queries on an invocation of
/// `Kademlia::poll`.
pub const JOBS_MAX_QUERIES: usize = 100;

/// The maximum number of new queries started by a background job
/// per invocation of `Kademlia::poll`.
pub const JOBS_MAX_NEW_QUERIES: usize = 10;

/// A background job run periodically.
#[derive(Debug)]
struct PeriodicJob<T> {
    interval: Duration,
    state: PeriodicJobState<T>,
}

impl<T> PeriodicJob<T> {
    fn is_running(&self) -> bool {
        match self.state {
            PeriodicJobState::Running(..) => true,
            PeriodicJobState::Waiting(..) => false,
        }
    }

    /// Cuts short the remaining delay, if the job is currently waiting
    /// for the delay to expire.
    fn asap(&mut self) {
        if let PeriodicJobState::Waiting(delay) = &mut self.state {
            delay.reset(Instant::now() - Duration::from_secs(1))
        }
    }

    /// Returns `true` if the job is currently not running but ready
    /// to be run, `false` otherwise.
    fn is_ready(&mut self, now: Instant) -> bool {
        if let PeriodicJobState::Waiting(delay) = &mut self.state {
            if now >= delay.deadline() || delay.poll().map(|a| a.is_ready()).unwrap_or(false) {
                return true
            }
        }
        false
    }
}

/// The state of a background job run periodically.
#[derive(Debug)]
enum PeriodicJobState<T> {
    Running(T),
    Waiting(Delay)
}

//////////////////////////////////////////////////////////////////////////////
// PutRecordJob

/// Periodic job for replicating / publishing records.
pub struct PutRecordJob {
    local_id: PeerId,
    next_publish: Option<Instant>,
    publish_interval: Option<Duration>,
    record_ttl: Option<Duration>,
    skipped: HashSet<Multihash>,
    inner: PeriodicJob<vec::IntoIter<Record>>,
}

impl PutRecordJob {
    /// Creates a new periodic job for replicating and re-publishing
    /// locally stored records.
    pub fn new(
        local_id: PeerId,
        replicate_interval: Duration,
        publish_interval: Option<Duration>,
        record_ttl: Option<Duration>,
    ) -> Self {
        let now = Instant::now();
        let delay = Delay::new(now + replicate_interval);
        let next_publish = publish_interval.map(|i| now + i);
        Self {
            local_id,
            next_publish,
            publish_interval,
            record_ttl,
            skipped: HashSet::new(),
            inner: PeriodicJob {
                interval: replicate_interval,
                state: PeriodicJobState::Waiting(delay)
            }
        }
    }

    /// Adds the key of a record that is ignored on the current or
    /// next run of the job.
    pub fn skip(&mut self, key: Multihash) {
        self.skipped.insert(key);
    }

    /// Checks whether the job is currently running.
    pub fn is_running(&self) -> bool {
        self.inner.is_running()
    }

    /// Cuts short the remaining delay, if the job is currently waiting
    /// for the delay to expire.
    ///
    /// The job is guaranteed to run on the next invocation of `poll`.
    pub fn asap(&mut self, publish: bool) {
        if publish {
            self.next_publish = Some(Instant::now() - Duration::from_secs(1))
        }
        self.inner.asap()
    }

    /// Polls the job for records to replicate.
    ///
    /// Must be called in the context of a task. When `NotReady` is returned,
    /// the current task is registered to be notified when the job is ready
    /// to be run.
    pub fn poll<T>(&mut self, store: &mut T, now: Instant) -> Async<Record>
    where
        for<'a> T: RecordStore<'a>
    {
        if self.inner.is_ready(now) {
            let publish = self.next_publish.map_or(false, |t_pub| now >= t_pub);
            let records = store.records()
                .filter_map(|r| {
                    let is_publisher = r.publisher.as_ref() == Some(&self.local_id);
                    if self.skipped.contains(&r.key) || (!publish && is_publisher) {
                        None
                    } else {
                        let mut record = r.into_owned();
                        if publish && is_publisher {
                            record.expires = record.expires.or_else(||
                                self.record_ttl.map(|ttl| now + ttl));
                        }
                        Some(record)
                    }
                })
                .collect::<Vec<_>>()
                .into_iter();

            // Schedule the next publishing run.
            if publish {
                self.next_publish = self.publish_interval.map(|i| now + i);
            }

            self.skipped.clear();

            self.inner.state = PeriodicJobState::Running(records);
        }

        if let PeriodicJobState::Running(records) = &mut self.inner.state {
            loop {
                if let Some(r) = records.next() {
                    if r.is_expired(now) {
                        store.remove(&r.key)
                    } else {
                        return Async::Ready(r)
                    }
                } else {
                    break
                }
            }

            // Wait for the next run.
            let delay = Delay::new(now + self.inner.interval);
            self.inner.state = PeriodicJobState::Waiting(delay);
            assert!(!self.inner.is_ready(now));
        }

        Async::NotReady
    }
}

//////////////////////////////////////////////////////////////////////////////
// AddProviderJob

/// Periodic job for replicating provider records.
pub struct AddProviderJob {
    inner: PeriodicJob<vec::IntoIter<ProviderRecord>>
}

impl AddProviderJob {
    /// Creates a new periodic job for provider announcements.
    pub fn new(interval: Duration) -> Self {
        let now = Instant::now();
        Self {
            inner: PeriodicJob {
                interval,
                state: PeriodicJobState::Waiting(Delay::new(now + interval))
            }
        }
    }

    /// Checks whether the job is currently running.
    pub fn is_running(&self) -> bool {
        self.inner.is_running()
    }

    /// Cuts short the remaining delay, if the job is currently waiting
    /// for the delay to expire.
    ///
    /// The job is guaranteed to run on the next invocation of `poll`.
    pub fn asap(&mut self) {
        self.inner.asap()
    }

    /// Polls the job for provider records to replicate.
    ///
    /// Must be called in the context of a task. When `NotReady` is returned,
    /// the current task is registered to be notified when the job is ready
    /// to be run.
    pub fn poll<T>(&mut self, store: &mut T, now: Instant) -> Async<ProviderRecord>
    where
        for<'a> T: RecordStore<'a>
    {
        if self.inner.is_ready(now) {
            let records = store.provided()
                .map(|r| r.into_owned())
                .collect::<Vec<_>>()
                .into_iter();
            self.inner.state = PeriodicJobState::Running(records);
        }

        if let PeriodicJobState::Running(keys) = &mut self.inner.state {
            loop {
                if let Some(r) = keys.next() {
                    if r.is_expired(now) {
                        store.remove_provider(&r.key, &r.provider)
                    } else {
                        return Async::Ready(r)
                    }
                } else {
                    break
                }
            }

            let delay = Delay::new(now + self.inner.interval);
            self.inner.state = PeriodicJobState::Waiting(delay);
            assert!(!self.inner.is_ready(now));
        }

        Async::NotReady
    }
}

#[cfg(test)]
mod tests {
    use crate::record::store::memory::MemoryStore;
    use quickcheck::*;
    use rand::Rng;
    use super::*;

    fn rand_put_record_job() -> PutRecordJob {
        let mut rng = rand::thread_rng();
        let id = PeerId::random();
        let replicate_interval = Duration::from_secs(rng.gen_range(1, 60));
        let publish_interval = Some(replicate_interval * rng.gen_range(1, 10));
        let record_ttl = Some(Duration::from_secs(rng.gen_range(1, 600)));
        PutRecordJob::new(id.clone(), replicate_interval, publish_interval, record_ttl)
    }

    fn rand_add_provider_job() -> AddProviderJob {
        let mut rng = rand::thread_rng();
        let interval = Duration::from_secs(rng.gen_range(1, 60));
        AddProviderJob::new(interval)
    }

    #[test]
    fn new_job_not_running() {
        let job = rand_put_record_job();
        assert!(!job.is_running());
        let job = rand_add_provider_job();
        assert!(!job.is_running());
    }

    #[test]
    fn run_put_record_job() {
        fn prop(records: Vec<Record>) {
            let mut job = rand_put_record_job();
            // Fill a record store.
            let mut store = MemoryStore::new(job.local_id.clone());
            for r in records {
                let _ = store.put(r);
            }
            // Polling with an instant beyond the deadline for the next run
            // is guaranteed to run the job, without the job needing to poll the `Delay`
            // and thus without needing to run `poll` in the context of a task
            // for testing purposes.
            let now = Instant::now() + job.inner.interval;
            // All (non-expired) records in the store must be yielded by the job.
            for r in store.records().map(|r| r.into_owned()).collect::<Vec<_>>() {
                if !r.is_expired(now) {
                    assert_eq!(job.poll(&mut store, now), Async::Ready(r));
                    assert!(job.is_running());
                }
            }
            assert_eq!(job.poll(&mut store, now), Async::NotReady);
            assert!(!job.is_running());
        }

        quickcheck(prop as fn(_))
    }

    #[test]
    fn run_add_provider_job() {
        fn prop(records: Vec<ProviderRecord>) {
            let mut job = rand_add_provider_job();
            let id = PeerId::random();
            // Fill a record store.
            let mut store = MemoryStore::new(id.clone());
            for mut r in records {
                r.provider = id.clone();
                let _ = store.add_provider(r);
            }
            // Polling with an instant beyond the deadline for the next run
            // is guaranteed to run the job, without the job needing to poll the `Delay`
            // and thus without needing to run `poll` in the context of a task
            // for testing purposes.
            let now = Instant::now() + job.inner.interval;
            // All (non-expired) records in the store must be yielded by the job.
            for r in store.provided().map(|r| r.into_owned()).collect::<Vec<_>>() {
                if !r.is_expired(now) {
                    assert_eq!(job.poll(&mut store, now), Async::Ready(r));
                    assert!(job.is_running());
                }
            }
            assert_eq!(job.poll(&mut store, now), Async::NotReady);
            assert!(!job.is_running());
        }

        quickcheck(prop as fn(_))
    }
}

