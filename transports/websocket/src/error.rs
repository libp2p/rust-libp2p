use std::{error, fmt};

/// Error in WebSockets.
#[derive(Debug)]
pub enum Error<E> {
    /// Error in the transport layer underneath.
    Transport(E),
    /// Websocket handshake error.
    Handshake(Box<dyn error::Error + Send>),
    /// Websocket base framing error.
    Base(Box<dyn error::Error + Send>)
}

impl<E: fmt::Display> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Transport(err) => write!(f, "{}", err),
            Error::Handshake(err) => write!(f, "{}", err),
            Error::Base(err) => write!(f, "{}", err)
        }
    }
}

impl<E: error::Error + 'static> error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Transport(err) => Some(err),
            Error::Handshake(err) => Some(&**err),
            Error::Base(err) => Some(&**err)
        }
    }
}

