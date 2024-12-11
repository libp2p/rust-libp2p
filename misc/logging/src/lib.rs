use tracing_subscriber::EnvFilter;

pub fn init_tracing_subscriber_with_default_env_filter() {
    init_tracing_subscriber_with_env_filter(EnvFilter::from_default_env());
}

pub fn init_tracing_subscriber_with_env_filter(filter: impl Into<EnvFilter>) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
