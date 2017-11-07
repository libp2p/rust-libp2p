extern crate bytes;
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;

mod dialer_select;
mod error;
mod listener_select;
mod tests;

pub mod protocol;

pub use self::dialer_select::dialer_select_proto;
pub use self::error::ProtocolChoiceError;
pub use self::listener_select::listener_select_proto;
