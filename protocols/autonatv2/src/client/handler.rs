// two handlers, share state in behaviour
// do isolated stuff in async function
//
// write basic tests
// Take a look at rendezvous
// TODO: tests
// TODO: Handlers

mod dial_back;
mod request;

use std::time::Duration;

use dial_back::Handler as DialBackHandler;
use libp2p_swarm::{ConnectionHandler, ConnectionHandlerSelect};
use request::Handler as RequestHandler;

pub use dial_back::ToBehaviour as DialBackToBehaviour;
pub use request::{
    Error as RequestError, FromBehaviour as RequestFromBehaviour, TestEnd,
    ToBehaviour as RequestToBehaviour,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_REQUESTS: usize = 10;

pub type Handler = ConnectionHandlerSelect<RequestHandler, DialBackHandler>;

pub fn new_handler() -> Handler {
    RequestHandler::new().select(DialBackHandler::new())
}
