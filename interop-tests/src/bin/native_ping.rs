use std::time::Duration;

use anyhow::Result;

mod config;

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::from_env()?;

    interop_tests::run_test(
        config.transport.parse()?,
        &config.ip,
        config.is_dialer,
        Duration::from_secs(config.test_timeout),
        &config.redis_addr,
    )
    .await
}
