use anyhow::Result;

mod config;

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::from_env()?;

    let result = interop_tests::run_test(
        &config.transport,
        &config.ip,
        config.is_dialer,
        config.test_timeout,
        &config.redis_addr,
    )
    .await?;

    println!("{result}");

    Ok(())
}
