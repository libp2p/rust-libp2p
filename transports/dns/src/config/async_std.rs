use std::{io, sync::Arc};

use async_std_resolver::AsyncStdResolver;
use parking_lot::Mutex;
use trust_dns_resolver::{
    config::{ResolverConfig, ResolverOpts},
    system_conf,
};

/// A `Transport` wrapper for performing DNS lookups when dialing `Multiaddr`esses
/// using `async-std` for all async I/O.
pub type Config<T> = crate::Config<T, AsyncStdResolver>;

impl<T> Config<T> {
    /// Creates a new [`Config`] from the OS's DNS configuration and defaults.
    pub async fn system(inner: T) -> Result<Config<T>, io::Error> {
        let (cfg, opts) = system_conf::read_system_conf()?;
        Self::custom(inner, cfg, opts).await
    }

    /// Creates a [`Config`] with a custom resolver configuration and options.
    pub async fn custom(
        inner: T,
        cfg: ResolverConfig,
        opts: ResolverOpts,
    ) -> Result<Config<T>, io::Error> {
        Ok(Config {
            inner: Arc::new(Mutex::new(inner)),
            resolver: async_std_resolver::resolver(cfg, opts).await,
        })
    }
}
