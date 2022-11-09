use libp2p_core::Executor;
use std::{future::Future, pin::Pin};

#[cfg(feature = "tokio")]
#[derive(Default, Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct TokioExecutor;

#[cfg(feature = "tokio")]
impl Executor for TokioExecutor {
    fn exec(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        let _ = tokio::spawn(future);
    }
}

#[cfg(feature = "async-std")]
#[derive(Default, Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct AsyncStdExecutor;

#[cfg(feature = "async-std")]
impl Executor for AsyncStdExecutor {
    fn exec(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        let _ = async_std::task::spawn(future);
    }
}
