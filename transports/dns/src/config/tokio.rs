use std::sync::Arc;

use parking_lot::Mutex;
use trust_dns_resolver::{system_conf, TokioAsyncResolver};

/// A `Transport` wrapper for performing DNS lookups when dialing `Multiaddr`esses
/// using `tokio` for all async I/O.
pub type Config<T> = crate::Config<T, TokioAsyncResolver>;

impl<T> Config<T> {
    /// Creates a new [`Config`] from the OS's DNS configuration and defaults.
    pub fn system(inner: T) -> Result<crate::Config<T, TokioAsyncResolver>, std::io::Error> {
        let (cfg, opts) = system_conf::read_system_conf()?;
        Self::custom(inner, cfg, opts)
    }

    /// Creates a [`Config`] with a custom resolver configuration
    /// and options.
    pub fn custom(
        inner: T,
        cfg: trust_dns_resolver::config::ResolverConfig,
        opts: trust_dns_resolver::config::ResolverOpts,
    ) -> Result<crate::Config<T, TokioAsyncResolver>, std::io::Error> {
        // TODO: Make infallible in next breaking release. Or deprecation?
        Ok(Config {
            inner: Arc::new(Mutex::new(inner)),
            resolver: TokioAsyncResolver::tokio(cfg, opts),
        })
    }
}
