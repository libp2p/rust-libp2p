use anyhow::Result;

mod config;

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::from_env()?;

    let report = interop_tests::run_test(
        &config.transport,
        &config.ip,
        config.is_dialer,
        config.test_timeout,
        &config.redis_addr,
        config.sec_protocol,
        config.muxer,
    )
    .await?;

    println!("{}", serde_json::to_string(&report)?);

    Ok(())
}
