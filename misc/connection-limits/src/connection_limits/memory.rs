// Copyright 2023 Protocol Labs.
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

use super::*;
use parking_lot::RwLock;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// The configurable memory usage based connection limits.
#[derive(Debug)]
pub struct MemoryUsageBasedConnectionLimits {
    max_process_memory_usage_bytes: Option<usize>,
    max_process_memory_usage_percentage: Option<f64>,
    system_physical_memory_bytes: usize,
    mem_tracker: ProcessMemoryUsageTracker,
}

impl MemoryUsageBasedConnectionLimits {
    pub fn new() -> Self {
        use sysinfo::{RefreshKind, SystemExt};

        let system_info = sysinfo::System::new_with_specifics(RefreshKind::new().with_memory());

        MemoryUsageBasedConnectionLimits {
            max_process_memory_usage_bytes: None,
            max_process_memory_usage_percentage: None,
            system_physical_memory_bytes: system_info.total_memory() as usize,
            mem_tracker: ProcessMemoryUsageTracker::new(),
        }
    }

    /// Sets the process memory usage threshold in bytes,
    /// all pending connections will be dropped when the threshold is exeeded
    pub fn with_max_bytes(mut self, bytes: usize) -> Self {
        self.max_process_memory_usage_bytes = Some(bytes);
        self
    }

    /// Sets the process memory usage threshold in the percentage of the total physical memory,
    /// all pending connections will be dropped when the threshold is exeeded
    pub fn with_max_percentage(mut self, percentage: f64) -> Self {
        self.max_process_memory_usage_percentage = Some(percentage);
        self
    }

    fn max_allowed_bytes(&self) -> Option<usize> {
        self.max_process_memory_usage_bytes.min(
            self.max_process_memory_usage_percentage
                .map(|p| (self.system_physical_memory_bytes as f64 * p).round() as usize),
        )
    }
}

impl Default for MemoryUsageBasedConnectionLimits {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionLimitsChecker for MemoryUsageBasedConnectionLimits {
    fn check_limit(&self, kind: ConnectionKind, _current: usize) -> Result<(), ConnectionDenied> {
        use ConnectionKind::*;

        match kind {
            PendingIncoming | PendingOutgoing => {
                if let Some(max_allowed_bytes) = self.max_allowed_bytes() {
                    self.mem_tracker.refresh_if_needed();
                    let process_memory_bytes = self.mem_tracker.total_bytes();
                    if process_memory_bytes > max_allowed_bytes {
                        return Err(ConnectionDenied::new(MemoryUsageLimitExceeded {
                            process_memory_bytes,
                            max_allowed_bytes,
                        }));
                    }
                }
            }
            EstablishedIncoming | EstablishedOutgoing | EstablishedPerPeer | EstablishedTotal => {}
        };

        Ok(())
    }
}

/// A connection limit has been exceeded.
#[derive(Debug, Clone, Copy)]
pub struct MemoryUsageLimitExceeded {
    pub process_memory_bytes: usize,
    pub max_allowed_bytes: usize,
}

impl std::error::Error for MemoryUsageLimitExceeded {}

impl fmt::Display for MemoryUsageLimitExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "pending connections are dropped, process memory usage limit exceeded: process memory: {} bytes, max allowed: {} bytes",
            self.process_memory_bytes,
            self.max_allowed_bytes,
        )
    }
}

#[derive(Debug)]
struct ProcessMemoryUsageTracker(Arc<RwLock<ProcessMemoryUsageTrackerInner>>);

impl ProcessMemoryUsageTracker {
    fn new() -> Self {
        Self(Arc::new(RwLock::new(ProcessMemoryUsageTrackerInner::new())))
    }

    fn refresh_if_needed(&self) {
        let need_refresh = self.0.read().need_refresh();
        if need_refresh {
            self.0.write().refresh();
        }
    }

    fn total_bytes(&self) -> usize {
        let guard = self.0.read();
        guard.physical_bytes + guard.virtual_bytes
    }
}

#[derive(Debug)]
struct ProcessMemoryUsageTrackerInner {
    last_checked: Instant,
    physical_bytes: usize,
    virtual_bytes: usize,
}

impl ProcessMemoryUsageTrackerInner {
    fn new() -> Self {
        let stats = memory_stats::memory_stats();
        if stats.is_none() {
            log::warn!("Failed to retrive process memory stats");
        }
        Self {
            last_checked: Instant::now(),
            physical_bytes: stats.map(|s| s.physical_mem).unwrap_or_default(),
            virtual_bytes: stats
                .map(|s: memory_stats::MemoryStats| s.virtual_mem)
                .unwrap_or_default(),
        }
    }

    fn need_refresh(&self) -> bool {
        const PROCESS_MEMORY_USAGE_CHECK_INTERVAL: Duration = Duration::from_millis(1000);

        self.last_checked + PROCESS_MEMORY_USAGE_CHECK_INTERVAL < Instant::now()
    }

    fn refresh(&mut self) {
        if let Some(stats) = memory_stats::memory_stats() {
            self.physical_bytes = stats.physical_mem;
            self.virtual_bytes = stats.virtual_mem;
        } else {
            log::warn!("Failed to retrive process memory stats");
        }
        self.last_checked = Instant::now();
    }
}
