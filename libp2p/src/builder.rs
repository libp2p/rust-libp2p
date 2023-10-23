use std::marker::PhantomData;

mod phase;
mod select_muxer;
mod select_security;

/// Build a [`Swarm`](libp2p_swarm::Swarm) by combining an identity, a set of
/// [`Transport`](libp2p_core::Transport)s and a
/// [`NetworkBehaviour`](libp2p_swarm::NetworkBehaviour).
///
/// ```
/// # use libp2p::{swarm::NetworkBehaviour, SwarmBuilder};
/// # use libp2p::core::transport::dummy::DummyTransport;
/// # use libp2p::core::muxing::StreamMuxerBox;
/// # use libp2p::identity::PeerId;
/// # use std::error::Error;
/// #
/// # #[cfg(all(
/// #     not(target_arch = "wasm32"),
/// #     feature = "tokio",
/// #     feature = "tcp",
/// #     feature = "tls",
/// #     feature = "noise",
/// #     feature = "quic",
/// #     feature = "dns",
/// #     feature = "relay",
/// #     feature = "websocket",
/// # ))]
/// # async fn build_swarm() -> Result<(), Box<dyn Error>> {
/// #     #[derive(NetworkBehaviour)]
/// #     #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
/// #     struct MyBehaviour {
/// #         relay: libp2p_relay::client::Behaviour,
/// #     }
///
///  let swarm = SwarmBuilder::with_new_identity()
///      .with_tokio()
///      .with_tcp(
///          Default::default(),
///          (libp2p_tls::Config::new, libp2p_noise::Config::new),
///          libp2p_yamux::Config::default,
///      )?
///      .with_quic()
///      .with_other_transport(|_key| DummyTransport::<(PeerId, StreamMuxerBox)>::new())?
///      .with_dns()?
///      .with_websocket(
///          (libp2p_tls::Config::new, libp2p_noise::Config::new),
///          libp2p_yamux::Config::default,
///      )
///      .await?
///      .with_relay_client(
///          (libp2p_tls::Config::new, libp2p_noise::Config::new),
///          libp2p_yamux::Config::default,
///      )?
///      .with_behaviour(|_key, relay| MyBehaviour { relay })?
///      .build();
/// #
/// #     Ok(())
/// # }
/// ```
pub struct SwarmBuilder<Provider, Phase> {
    keypair: libp2p_identity::Keypair,
    phantom: PhantomData<Provider>,
    phase: Phase,
}

#[cfg(test)]
mod tests {
    use crate::SwarmBuilder;
    use libp2p_core::{muxing::StreamMuxerBox, transport::dummy::DummyTransport};
    use libp2p_identity::PeerId;
    use libp2p_swarm::{NetworkBehaviour, Swarm};

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "tls", feature = "noise"))]
    fn tcp() {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                libp2p_yamux::Config::default,
            )
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(
        feature = "async-std",
        feature = "tcp",
        feature = "tls",
        feature = "noise"
    ))]
    fn async_std_tcp() {
        let _ = SwarmBuilder::with_new_identity()
            .with_async_std()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                libp2p_yamux::Config::default,
            )
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "tls", feature = "mplex"))]
    fn tcp_yamux_mplex() {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                (
                    libp2p_yamux::Config::default(),
                    libp2p_mplex::MplexConfig::default(),
                ),
            )
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "tls", feature = "noise"))]
    fn tcp_tls_noise() {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                (libp2p_tls::Config::new, libp2p_noise::Config::new),
                (
                    libp2p_yamux::Config::default,
                    libp2p_mplex::MplexConfig::default,
                ),
            )
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "quic"
    ))]
    fn tcp_quic() {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                (libp2p_tls::Config::new, libp2p_noise::Config::new),
                libp2p_yamux::Config::default,
            )
            .unwrap()
            .with_quic()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "relay"
    ))]
    fn tcp_relay() {
        #[derive(libp2p_swarm::NetworkBehaviour)]
        #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
        struct Behaviour {
            dummy: libp2p_swarm::dummy::Behaviour,
            relay: libp2p_relay::client::Behaviour,
        }

        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                libp2p_yamux::Config::default,
            )
            .unwrap()
            .with_relay_client(libp2p_tls::Config::new, libp2p_yamux::Config::default)
            .unwrap()
            .with_behaviour(|_, relay| Behaviour {
                dummy: libp2p_swarm::dummy::Behaviour,
                relay,
            })
            .unwrap()
            .build();
    }

    #[tokio::test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "dns"
    ))]
    async fn tcp_dns() {
        SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                (libp2p_tls::Config::new, libp2p_noise::Config::new),
                libp2p_yamux::Config::default,
            )
            .unwrap()
            .with_dns()
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    /// Showcases how to provide custom transports unknown to the libp2p crate, e.g. WebRTC.
    #[test]
    #[cfg(feature = "tokio")]
    fn other_transport() -> Result<(), Box<dyn std::error::Error>> {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            // Closure can either return a Transport directly.
            .with_other_transport(|_| DummyTransport::<(PeerId, StreamMuxerBox)>::new())?
            // Or a Result containing a Transport.
            .with_other_transport(|_| {
                if true {
                    Ok(DummyTransport::<(PeerId, StreamMuxerBox)>::new())
                } else {
                    Err(Box::from("test"))
                }
            })?
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();

        Ok(())
    }

    #[tokio::test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "dns",
        feature = "websocket",
    ))]
    async fn tcp_websocket() {
        let _ = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                (libp2p_tls::Config::new, libp2p_noise::Config::new),
                libp2p_yamux::Config::default,
            )
            .unwrap()
            .with_websocket(
                (libp2p_tls::Config::new, libp2p_noise::Config::new),
                libp2p_yamux::Config::default,
            )
            .await
            .unwrap()
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();
    }

    #[tokio::test]
    #[cfg(all(
        feature = "tokio",
        feature = "tcp",
        feature = "tls",
        feature = "noise",
        feature = "quic",
        feature = "dns",
        feature = "relay",
        feature = "websocket",
    ))]
    async fn all() {
        #[derive(NetworkBehaviour)]
        #[behaviour(prelude = "libp2p_swarm::derive_prelude")]
        struct MyBehaviour {
            relay: libp2p_relay::client::Behaviour,
        }

        let (builder, _bandwidth_sinks) = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                libp2p_yamux::Config::default,
            )
            .unwrap()
            .with_quic()
            .with_dns()
            .unwrap()
            .with_websocket(libp2p_tls::Config::new, libp2p_yamux::Config::default)
            .await
            .unwrap()
            .with_relay_client(libp2p_tls::Config::new, libp2p_yamux::Config::default)
            .unwrap()
            .with_bandwidth_logging();
        let _: Swarm<MyBehaviour> = builder
            .with_behaviour(|_key, relay| MyBehaviour { relay })
            .unwrap()
            .build();
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "tcp", feature = "tls", feature = "yamux"))]
    fn tcp_bandwidth_logging() -> Result<(), Box<dyn std::error::Error>> {
        let (builder, _logging) = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                Default::default(),
                libp2p_tls::Config::new,
                libp2p_yamux::Config::default,
            )?
            .with_bandwidth_logging();

        builder
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();

        Ok(())
    }

    #[test]
    #[cfg(all(feature = "tokio", feature = "quic"))]
    fn quic_bandwidth_logging() -> Result<(), Box<dyn std::error::Error>> {
        let (builder, _logging) = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_quic()
            .with_bandwidth_logging();

        builder
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();

        Ok(())
    }

    #[test]
    #[cfg(feature = "tokio")]
    fn other_transport_bandwidth_logging() -> Result<(), Box<dyn std::error::Error>> {
        let (builder, _logging) = SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_other_transport(|_| DummyTransport::<(PeerId, StreamMuxerBox)>::new())?
            .with_bandwidth_logging();

        builder
            .with_behaviour(|_| libp2p_swarm::dummy::Behaviour)
            .unwrap()
            .build();

        Ok(())
    }
}
