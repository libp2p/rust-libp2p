//! Automatic TLS certificate provisioning for libp2p (AutoTLS).
//!
//! AutoTLS lets a publicly reachable node automatically obtain a browser-trusted
//! [Let's Encrypt](https://letsencrypt.org) wildcard certificate for
//! `*.<PeerID>.libp2p.direct`, so that browsers can dial it over Secure WebSockets (WSS).
//!
//! The certificate is obtained through the ACME `dns-01` challenge: the node proves control of
//! its `PeerId` to the `registration.libp2p.direct` broker, which then publishes the
//! `_acme-challenge` TXT record on its behalf. The `libp2p.direct` authoritative DNS encodes the
//! node's public IP directly into the queried hostname (`1-2-3-4.<PeerID>.libp2p.direct` resolves
//! to `1.2.3.4`), so a single wildcard certificate covers every per-IP hostname and survives IP
//! changes.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

#[cfg(feature = "tokio")]
pub mod acme;
#[cfg(feature = "tokio")]
mod behaviour;
pub mod broker;
pub mod cert;
mod cert_resolver;
pub mod encoding;
pub mod peer_id_auth;
pub mod storage;

#[cfg(feature = "tokio")]
pub use behaviour::{Behaviour, Event};
pub use cert_resolver::AutoTlsCertResolver;
