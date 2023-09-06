mod bounded;
mod unique;

pub use bounded::BoundedWorkers;
pub use unique::{PushError, UniqueWorkers};

#[derive(Debug)]
pub struct Timeout {
    _priv: (),
}

impl Timeout {
    fn new() -> Self {
        Self { _priv: () }
    }
}
