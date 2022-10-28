use libp2p_yamux::YamuxConfig;

#[async_std::test]
async fn close_implies_flush() {
    let (alice, bob) =
        libp2p_muxer_test_harness::connected_muxers_on_memory_transport::<YamuxConfig, _, _>()
            .await;

    libp2p_muxer_test_harness::close_implies_flush(alice, bob).await;
}

#[async_std::test]
#[ignore] // Hangs forever, is this a harness bug? It passes if we try to write to the stream.
async fn dialer_can_receive() {
    let (alice, bob) =
        libp2p_muxer_test_harness::connected_muxers_on_memory_transport::<YamuxConfig, _, _>()
            .await;

    libp2p_muxer_test_harness::dialer_can_receive(alice, bob).await;
}

#[async_std::test]
async fn read_after_close() {
    let (alice, bob) =
        libp2p_muxer_test_harness::connected_muxers_on_memory_transport::<YamuxConfig, _, _>()
            .await;

    libp2p_muxer_test_harness::read_after_close(alice, bob).await;
}
