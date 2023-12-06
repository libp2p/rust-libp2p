// two handlers, share state in behaviour
// do isolated stuff in async function
//
// write basic tests
// Take a look at rendezvous
// TODO: tests
// TODO: Handlers

pub(crate) mod dial_back;
pub(crate) mod dial_request;

use either::Either;
use std::time::Duration;

pub(crate) use dial_request::TestEnd;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_REQUESTS: usize = 10;

pub(crate) type Handler = Either<dial_request::Handler, dial_back::Handler>;
