use either::Either;

pub(crate) mod dial_back;
pub(crate) mod dial_request;

pub type Handler<R> = Either<dial_back::Handler, dial_request::Handler<R>>;
