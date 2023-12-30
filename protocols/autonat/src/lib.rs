#[cfg(feature = "v1")]
pub mod v1;

#[cfg(feature = "v2")]
pub mod v2;

#[cfg(feature = "v1")]
#[deprecated(since = "0.13.0", note = "Please use `v1` module instead.")]
pub use v1::*;
