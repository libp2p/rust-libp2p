#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .parse_filters("webrtc_example_server=debug,libp2p_webrtc=info,libp2p_ping=debug")
        .parse_default_env()
        .init();

    webrtc_example_server::start().await?;

    Ok(())
}
