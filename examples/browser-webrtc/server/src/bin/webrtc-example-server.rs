#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let remote = std::env::args()
        .nth(1)
        .map(|addr| addr.parse())
        .transpose()?;

    webrtc_example_server::start(remote).await?;

    Ok(())
}
