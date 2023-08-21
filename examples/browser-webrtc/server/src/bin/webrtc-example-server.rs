#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    webrtc_example_server::start().await?;

    Ok(())
}
