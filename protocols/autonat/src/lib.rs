#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[cfg(feature = "v1")]
pub mod v1;

#[cfg(feature = "v2")]
pub mod v2;

#[cfg(feature = "v1")]
pub use v1::*;
