use anyhow::Result;

mod config;

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::from_env()?;

    let report = interop_tests::run_test(
        &config.transport,
        &config.listener_ip,
        config.is_dialer,
        config.test_timeout_secs,
        &config.redis_addr,
        config.secure_channel,
        config.muxer,
        config.debug,
        &config.test_key,
    )
    .await?;

    println!("latency:");
    println!("  handshake_plus_one_rtt: {}", report.handshake_plus_one_rtt_ms);
    println!("  ping_rtt: {}", report.ping_rtt_ms);
    println!("  unit: ms");

    Ok(())
}
