pub use tracing_subscriber::EnvFilter;

/// Initializes logging with the default environment filter (`RUST_LOG`).
pub fn with_default_env_filter() {
    with_env_filter(EnvFilter::from_default_env());
}

/// Initializes logging with a custom environment filter.
/// Logs are written to standard error (`stderr`).
pub fn with_env_filter(filter: impl Into<EnvFilter>) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
