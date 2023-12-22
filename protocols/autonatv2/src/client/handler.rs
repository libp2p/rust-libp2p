pub(crate) mod dial_back;
pub(crate) mod dial_request;

use std::{
    fmt::{Display, Formatter},
    sync::Arc,
    time::Duration,
};

pub(crate) use dial_request::TestEnd;

use self::dial_request::InternalError;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_REQUESTS: usize = 10;

#[derive(Clone, Debug)]
pub struct Error {
    pub(crate) internal: Arc<InternalError>,
}

impl From<InternalError> for Error {
    fn from(value: InternalError) -> Self {
        Self {
            internal: Arc::new(value),
        }
    }
}

impl From<Arc<InternalError>> for Error {
    fn from(value: Arc<InternalError>) -> Self {
        Self { internal: value }
    }
}

impl From<&Arc<InternalError>> for Error {
    fn from(value: &Arc<InternalError>) -> Self {
        Self {
            internal: Arc::clone(value),
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.internal.fmt(f)
    }
}
