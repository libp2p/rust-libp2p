use std::env;

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub(crate) struct Config {
    pub(crate) transport: String,
    pub(crate) secure_channel: Option<String>,
    pub(crate) muxer: Option<String>,
    pub(crate) listener_ip: String,
    pub(crate) is_dialer: bool,
    pub(crate) test_timeout_secs: u64,
    pub(crate) redis_addr: String,
    pub(crate) debug: bool,
    pub(crate) test_key: String,
}

impl Config {
    pub(crate) fn from_env() -> Result<Self> {
        let debug = env::var("DEBUG")
            .unwrap_or_else(|_| "false".into())
            .parse::<bool>()?;
        let transport =
            env::var("TRANSPORT").context("TRANSPORT environment variable is not set")?;
        let listener_ip = env::var("LISTENER_IP").context("LISTENER_IP environment variable is not set")?;
        let test_key = env::var("TEST_KEY").context("TEST_KEY environment variable is not set")?;
        let is_dialer = env::var("IS_DIALER")
            .unwrap_or_else(|_| "true".into())
            .parse::<bool>()?;
        let test_timeout_secs = env::var("TEST_TIMEOUT_SECS")
            .unwrap_or_else(|_| "180".into())
            .parse::<u64>()?;
        let redis_addr = env::var("REDIS_ADDR")
            .map(|addr| format!("redis://{addr}"))
            .unwrap_or_else(|_| "redis://redis:6379".into());

        let secure_channel = env::var("SECURE_CHANNEL").ok();
        let muxer = env::var("MUXER").ok();

        Ok(Self {
            transport,
            secure_channel,
            muxer,
            listener_ip,
            is_dialer,
            test_timeout_secs,
            redis_addr,
            debug,
            test_key,
        })
    }
}
