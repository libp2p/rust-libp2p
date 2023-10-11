use std::env;

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) transport: String,
    pub(crate) sec_protocol: Option<String>,
    pub(crate) muxer: Option<String>,
    pub(crate) ip: String,
    pub(crate) is_dialer: bool,
    pub(crate) test_timeout: u64,
    pub(crate) redis_addr: String,
}

impl Config {
    pub(crate) fn from_env() -> Result<Self> {
        let transport =
            env::var("transport").context("transport environment variable is not set")?;
        let ip = env::var("ip").context("ip environment variable is not set")?;
        let is_dialer = env::var("is_dialer")
            .unwrap_or_else(|_| "true".into())
            .parse::<bool>()?;
        let test_timeout = env::var("test_timeout_seconds")
            .unwrap_or_else(|_| "180".into())
            .parse::<u64>()?;
        let redis_addr = env::var("redis_addr")
            .map(|addr| format!("redis://{addr}"))
            .unwrap_or_else(|_| "redis://redis:6379".into());

        let sec_protocol = env::var("security").ok();
        let muxer = env::var("muxer").ok();

        Ok(Self {
            transport,
            sec_protocol,
            muxer,
            ip,
            is_dialer,
            test_timeout,
            redis_addr,
        })
    }
}
