mod list;
mod map;

pub use list::FuturesList;
pub use map::{FuturesMap, PushError};

#[derive(Debug)]
pub struct Timeout {
    _priv: (),
}

impl Timeout {
    fn new() -> Self {
        Self { _priv: () }
    }
}
