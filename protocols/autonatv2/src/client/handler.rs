pub(crate) mod dial_back;
pub(crate) mod dial_request;

use std::time::Duration;

pub(crate) use dial_request::TestEnd;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_REQUESTS: usize = 10;
