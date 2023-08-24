use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env::set_var(
        "RUST_LOG",
        "debug,libp2p_webrtc,libp2p_ping=debug,webrtc_sctp=off,webrtc_mdns=off,webrtc_ice=off",
    );

    env_logger::init();

    webrtc_example_server::start().await?;

    Ok(())
}
