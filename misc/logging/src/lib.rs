use tracing_subscriber::EnvFilter;

pub fn with_env_filter(){
    let _ = tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::from_default_env())
    .with_writer(||std::io::stderr())
    .try_init();
}