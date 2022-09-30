// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

//! libp2p is a modular peer-to-peer networking framework.
//!
//! To learn more about the general libp2p multi-language framework visit
//! [libp2p.io](https://libp2p.io/).
//!
//! To get started with this libp2p implementation in Rust, please take a look
//! at the [`tutorials`](crate::tutorials). Further examples can be found in the
//! [examples] directory.
//!
//! [examples]: https://github.com/libp2p/rust-libp2p/tree/master/examples

#![doc(html_logo_url = "https://libp2p.io/img/logo_small.png")]
#![doc(html_favicon_url = "https://libp2p.io/img/favicon.png")]

pub use bytes;
pub use futures;
#[doc(inline)]
pub use libp2p_core::multihash;
#[doc(inline)]
pub use multiaddr;

#[cfg(feature = "autonat")]
#[cfg_attr(docsrs, doc(cfg(feature = "autonat")))]
#[doc(inline)]
pub use libp2p_autonat as autonat;
#[doc(inline)]
pub use libp2p_core as core;
#[cfg(feature = "dcutr")]
#[cfg_attr(docsrs, doc(cfg(feature = "dcutr")))]
#[doc(inline)]
pub use libp2p_dcutr as dcutr;
#[cfg(feature = "deflate")]
#[cfg_attr(docsrs, doc(cfg(feature = "deflate")))]
#[cfg(not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")))]
#[doc(inline)]
pub use libp2p_deflate as deflate;
#[deprecated(
    since = "0.49.0",
    note = "The `dns-tokio` and `dns-async-std` features are deprecated. Use the new `dns` feature together with the `tokio` or `async-std` features."
)]
#[cfg(all(
    any(feature = "dns-tokio", feature = "dns-async-std"),
    not(feature = "dns")
))]
#[cfg(not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")))]
#[cfg_attr(
    docsrs,
    doc(cfg(any(feature = "dns-tokio", feature = "dns-async-std")))
)]
pub mod dns {
    #[doc(inline)]
    pub use libp2p_dns::*;
}

#[cfg(feature = "dns")]
#[cfg(not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")))]
#[cfg_attr(docsrs, doc(cfg(feature = "dns")))]
pub mod dns {
    #[doc(inline)]
    pub use libp2p_dns::*;
}
#[cfg(feature = "floodsub")]
#[cfg_attr(docsrs, doc(cfg(feature = "floodsub")))]
#[doc(inline)]
pub use libp2p_floodsub as floodsub;
#[cfg(feature = "gossipsub")]
#[cfg_attr(docsrs, doc(cfg(feature = "gossipsub")))]
#[cfg(not(target_os = "unknown"))]
#[doc(inline)]
pub use libp2p_gossipsub as gossipsub;
#[cfg(feature = "identify")]
#[cfg_attr(docsrs, doc(cfg(feature = "identify")))]
#[doc(inline)]
pub use libp2p_identify as identify;
#[cfg(feature = "kad")]
#[cfg_attr(docsrs, doc(cfg(feature = "kad")))]
#[doc(inline)]
pub use libp2p_kad as kad;
#[deprecated(
    since = "0.49.0",
    note = "The `mdns-tokio` and `mdns-async-io` features are deprecated. Use the new `mdns` feature together with the `tokio` or `async-std` features."
)]
#[cfg(all(
    any(feature = "mdns-async-io", feature = "mdns-tokio"),
    not(feature = "mdns")
))]
#[cfg(not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")))]
#[cfg_attr(
    docsrs,
    doc(cfg(any(feature = "mdns-tokio", feature = "mdns-async-io")))
)]
pub mod mdns {
    #[doc(inline)]
    pub use libp2p_mdns::*;
}

#[cfg(feature = "mdns")]
#[cfg(not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")))]
#[cfg_attr(docsrs, doc(cfg(feature = "mdns")))]
pub mod mdns {
    #[doc(inline)]
    pub use libp2p_mdns::*;
}
#[cfg(feature = "metrics")]
#[cfg_attr(docsrs, doc(cfg(feature = "metrics")))]
#[doc(inline)]
pub use libp2p_metrics as metrics;
#[cfg(feature = "mplex")]
#[cfg_attr(docsrs, doc(cfg(feature = "mplex")))]
#[doc(inline)]
pub use libp2p_mplex as mplex;
#[cfg(feature = "noise")]
#[cfg_attr(docsrs, doc(cfg(feature = "noise")))]
#[doc(inline)]
pub use libp2p_noise as noise;
#[cfg(feature = "ping")]
#[cfg_attr(docsrs, doc(cfg(feature = "ping")))]
#[doc(inline)]
pub use libp2p_ping as ping;
#[cfg(feature = "plaintext")]
#[cfg_attr(docsrs, doc(cfg(feature = "plaintext")))]
#[doc(inline)]
pub use libp2p_plaintext as plaintext;
#[cfg(feature = "pnet")]
#[cfg_attr(docsrs, doc(cfg(feature = "pnet")))]
#[doc(inline)]
pub use libp2p_pnet as pnet;
#[cfg(feature = "relay")]
#[cfg_attr(docsrs, doc(cfg(feature = "relay")))]
#[doc(inline)]
pub use libp2p_relay as relay;
#[cfg(feature = "rendezvous")]
#[cfg_attr(docsrs, doc(cfg(feature = "rendezvous")))]
#[doc(inline)]
pub use libp2p_rendezvous as rendezvous;
#[cfg(feature = "request-response")]
#[cfg_attr(docsrs, doc(cfg(feature = "request-response")))]
#[doc(inline)]
pub use libp2p_request_response as request_response;
#[doc(inline)]
pub use libp2p_swarm as swarm;
#[deprecated(
    since = "0.49.0",
    note = "The `tcp-tokio` and `tcp-async-io` features are deprecated. Use the new `tcp` feature together with the `tokio` or `async-std` features."
)]
#[cfg(all(
    any(feature = "tcp-tokio", feature = "tcp-async-io"),
    not(feature = "tcp")
))]
#[cfg(not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "tcp-tokio", feature = "tcp-async-io"))))]
pub mod tcp {
    #[doc(inline)]
    pub use libp2p_tcp::*;
}

#[cfg(feature = "tcp")]
#[cfg(not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")))]
#[cfg_attr(docsrs, doc(cfg(feature = "tcp")))]
pub mod tcp {
    #[doc(inline)]
    pub use libp2p_tcp::*;
}
#[cfg(feature = "uds")]
#[cfg_attr(docsrs, doc(cfg(feature = "uds")))]
#[doc(inline)]
pub use libp2p_uds as uds;
#[cfg(feature = "wasm-ext")]
#[cfg_attr(docsrs, doc(cfg(feature = "wasm-ext")))]
#[doc(inline)]
pub use libp2p_wasm_ext as wasm_ext;
#[cfg(feature = "websocket")]
#[cfg_attr(docsrs, doc(cfg(feature = "websocket")))]
#[cfg(not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")))]
#[doc(inline)]
pub use libp2p_websocket as websocket;
#[cfg(feature = "yamux")]
#[cfg_attr(docsrs, doc(cfg(feature = "yamux")))]
#[doc(inline)]
pub use libp2p_yamux as yamux;

mod transport_ext;

pub mod bandwidth;
pub mod simple;

#[cfg(doc)]
pub mod tutorials;

pub use self::core::{
    identity,
    transport::TransportError,
    upgrade::{InboundUpgrade, InboundUpgradeExt, OutboundUpgrade, OutboundUpgradeExt},
    PeerId, Transport,
};
pub use self::multiaddr::{multiaddr as build_multiaddr, Multiaddr};
pub use self::simple::SimpleProtocol;
pub use self::swarm::Swarm;
pub use self::transport_ext::TransportExt;
pub use libp2p_swarm_derive::NetworkBehaviour;

/// Builds a `Transport` based on TCP/IP that supports the most commonly-used features of libp2p:
///
///  * DNS resolution.
///  * Noise protocol encryption.
///  * Websockets.
///  * Both Yamux and Mplex for substream multiplexing.
///
/// All async I/O of the transport is based on `async-std`.
///
/// > **Note**: This `Transport` is not suitable for production usage, as its implementation
/// >           reserves the right to support additional protocols or remove deprecated protocols.
#[cfg(all(
    not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")),
    any(
        all(feature = "tcp-async-io", feature = "dns-async-std"),
        all(feature = "tcp", feature = "dns", feature = "async-std")
    ),
    feature = "websocket",
    feature = "noise",
    feature = "mplex",
    feature = "yamux"
))]
#[cfg_attr(
    docsrs,
    doc(cfg(all(
        not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")),
        any(
            all(feature = "tcp-async-io", feature = "dns-async-std"),
            all(feature = "tcp", feature = "dns", feature = "async-std")
        ),
        feature = "websocket",
        feature = "noise",
        feature = "mplex",
        feature = "yamux"
    )))
)]
#[cfg_attr(
    all(
        any(feature = "tcp-async-io", feature = "dns-async-std"),
        not(feature = "async-std")
    ),
    deprecated(
        since = "0.49.0",
        note = "The `tcp-async-io` and `dns-async-std` features are deprecated. Use the new `tcp` and `dns` features together with the `async-std` feature."
    )
)]
pub async fn development_transport(
    keypair: identity::Keypair,
) -> std::io::Result<core::transport::Boxed<(PeerId, core::muxing::StreamMuxerBox)>> {
    let transport = {
        let dns_tcp = dns::DnsConfig::system(tcp::TcpTransport::new(
            tcp::GenTcpConfig::new().nodelay(true),
        ))
        .await?;
        let ws_dns_tcp = websocket::WsConfig::new(
            dns::DnsConfig::system(tcp::TcpTransport::new(
                tcp::GenTcpConfig::new().nodelay(true),
            ))
            .await?,
        );
        dns_tcp.or_transport(ws_dns_tcp)
    };

    Ok(transport
        .upgrade(core::upgrade::Version::V1)
        .authenticate(noise::NoiseAuthenticated::xx(&keypair).unwrap())
        .multiplex(core::upgrade::SelectUpgrade::new(
            yamux::YamuxConfig::default(),
            mplex::MplexConfig::default(),
        ))
        .timeout(std::time::Duration::from_secs(20))
        .boxed())
}

/// Builds a `Transport` based on TCP/IP that supports the most commonly-used features of libp2p:
///
///  * DNS resolution.
///  * Noise protocol encryption.
///  * Websockets.
///  * Both Yamux and Mplex for substream multiplexing.
///
/// All async I/O of the transport is based on `tokio`.
///
/// > **Note**: This `Transport` is not suitable for production usage, as its implementation
/// >           reserves the right to support additional protocols or remove deprecated protocols.
#[cfg(all(
    not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")),
    any(
        all(feature = "tcp-tokio", feature = "dns-tokio"),
        all(feature = "tcp", feature = "dns", feature = "tokio")
    ),
    feature = "websocket",
    feature = "noise",
    feature = "mplex",
    feature = "yamux"
))]
#[cfg_attr(
    docsrs,
    doc(cfg(all(
        not(any(target_os = "emscripten", target_os = "wasi", target_os = "unknown")),
        any(
            all(feature = "tcp-tokio", feature = "dns-tokio"),
            all(feature = "tcp", feature = "dns", feature = "tokio")
        ),
        feature = "websocket",
        feature = "noise",
        feature = "mplex",
        feature = "yamux"
    )))
)]
#[cfg_attr(
    all(
        any(feature = "tcp-tokio", feature = "dns-tokio"),
        not(feature = "tokio")
    ),
    deprecated(
        since = "0.49.0",
        note = "The `tcp-tokio` and `dns-tokio` features are deprecated. Use the new `tcp` and `dns` feature together with the `tokio` feature."
    )
)]
pub fn tokio_development_transport(
    keypair: identity::Keypair,
) -> std::io::Result<core::transport::Boxed<(PeerId, core::muxing::StreamMuxerBox)>> {
    let transport = {
        let dns_tcp = dns::TokioDnsConfig::system(tcp::TokioTcpTransport::new(
            tcp::GenTcpConfig::new().nodelay(true),
        ))?;
        let ws_dns_tcp = websocket::WsConfig::new(dns::TokioDnsConfig::system(
            tcp::TokioTcpTransport::new(tcp::GenTcpConfig::new().nodelay(true)),
        )?);
        dns_tcp.or_transport(ws_dns_tcp)
    };

    Ok(transport
        .upgrade(core::upgrade::Version::V1)
        .authenticate(noise::NoiseAuthenticated::xx(&keypair).unwrap())
        .multiplex(core::upgrade::SelectUpgrade::new(
            yamux::YamuxConfig::default(),
            mplex::MplexConfig::default(),
        ))
        .timeout(std::time::Duration::from_secs(20))
        .boxed())
}
