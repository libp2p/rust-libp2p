use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env::set_var(
        "RUST_LOG",
        "webrtc_example_server=debug,libp2p_webrtc=info,libp2p_ping=debug",
    );

    env_logger::init();

    webrtc_example_server::start().await?;

    Ok(())
}
