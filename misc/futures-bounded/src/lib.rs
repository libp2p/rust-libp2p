mod set;
mod map;

pub use set::FuturesSet;
pub use map::{FuturesMap, PushError};
use std::fmt;
use std::fmt::Formatter;
use std::time::Duration;

/// A future failed to complete within the given timeout.
#[derive(Debug)]
pub struct Timeout {
    limit: Duration,
}

impl Timeout {
    fn new(duration: Duration) -> Self {
        Self { limit: duration }
    }
}

impl fmt::Display for Timeout {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "future failed to complete within {:?}", self.limit)
    }
}

impl std::error::Error for Timeout {}
