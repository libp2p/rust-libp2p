use crate::nodes::handled_node::HandledNodeError;
use std::{fmt, error};

/// Error that can happen in a task.
#[derive(Debug)]
pub enum Error<R, H> {
    /// An error happend while we were trying to reach the node.
    Reach(R),
    /// An error happened after the node has been reached.
    Node(HandledNodeError<H>)
}

impl<R, H> fmt::Display for Error<R, H>
where
    R: fmt::Display,
    H: fmt::Display
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Reach(err) => write!(f, "reach error: {}", err),
            Error::Node(err) => write!(f, "node error: {}", err)
        }
    }
}

impl<R, H> error::Error for Error<R, H>
where
    R: error::Error + 'static,
    H: error::Error + 'static
{
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Reach(err) => Some(err),
            Error::Node(err) => Some(err)
        }
    }
}

