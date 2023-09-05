mod bounded;
mod unique;

pub use bounded::BoundedWorkers;
pub use unique::UniqueWorkers;

#[derive(Debug)]
pub struct Timeout {
    _priv: (),
}

impl Timeout {
    fn new() -> Self {
        Self { _priv: () }
    }
}

/// Result of a worker pushing
#[derive(PartialEq, Debug)]
pub enum PushResult {
    Ok,
    BeyondCapacity,
    ExistedID,
}

impl PushResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, PushResult::Ok)
    }

    pub fn is_failing(&self) -> bool {
        !self.is_ok()
    }
}
