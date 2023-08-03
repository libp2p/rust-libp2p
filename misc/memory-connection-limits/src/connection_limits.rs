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
use libp2p_swarm::ConnectionDenied;
use std::fmt;

/// The configurable memory usage based connection limits.
#[derive(Debug)]
pub struct MemoryUsageBasedConnectionLimits {
    max_process_memory_usage_bytes: Option<usize>,
    max_process_memory_usage_percentage: Option<f64>,
    system_physical_memory_bytes: usize,
}

impl MemoryUsageBasedConnectionLimits {
    pub fn new() -> Self {
        use sysinfo::{RefreshKind, SystemExt};

        let system_info = sysinfo::System::new_with_specifics(RefreshKind::new().with_memory());

        MemoryUsageBasedConnectionLimits {
            max_process_memory_usage_bytes: None,
            max_process_memory_usage_percentage: None,
            system_physical_memory_bytes: system_info.total_memory() as usize,
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

    pub(crate) fn check_limit(
        &self,
        mem_tracker: &mut ProcessMemoryUsageTracker,
    ) -> Result<(), ConnectionDenied> {
        if let Some(max_allowed_bytes) = self.max_allowed_bytes() {
            mem_tracker.refresh_if_needed();
            let process_memory_bytes = mem_tracker.total_bytes();
            if process_memory_bytes > max_allowed_bytes {
                return Err(ConnectionDenied::new(MemoryUsageLimitExceeded {
                    process_memory_bytes,
                    max_allowed_bytes,
                }));
            }
        }

        Ok(())
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
