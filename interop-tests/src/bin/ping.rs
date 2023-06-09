use std::{env, time::Duration};

use anyhow::{Context, Result};
use interop_tests::{from_env, Transport};

#[tokio::main]
async fn main() -> Result<()> {
    let transport: Transport = from_env("transport")?;
    let ip = env::var("ip").context("ip environment variable is not set")?;
    let is_dialer = env::var("is_dialer")
        .unwrap_or_else(|_| "true".into())
        .parse::<bool>()?;
    let test_timeout = Duration::from_secs(
        env::var("test_timeout_seconds")
            .unwrap_or_else(|_| "180".into())
            .parse::<u64>()?,
    );
    let redis_addr = env::var("redis_addr")
        .map(|addr| format!("redis://{addr}"))
        .unwrap_or_else(|_| "redis://redis:6379".into());

    println!(
        "{}",
        interop_tests::run_test(transport, &ip, is_dialer, test_timeout, &redis_addr).await?
    );
    Ok(())
}
