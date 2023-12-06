use wasm_bindgen::{JsCast, JsValue};

/// Errors that may happen on the [`Transport`](crate::Transport) or the
/// [`Connection`](crate::Connection).
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid multiaddr: {0}")]
    InvalidMultiaddr(&'static str),

    #[error("JavaScript error: {0}")]
    Js(String),

    #[error("JavaScript typecasting failed")]
    JsCastFailed,

    #[error("Unknown remote peer ID")]
    UnknownRemotePeerId,

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Authentication error")]
    Authentication(#[from] AuthenticationError),
}

/// New-type wrapper to hide `libp2p_noise` from the public API.
#[derive(thiserror::Error, Debug)]
#[error(transparent)]
pub struct AuthenticationError(pub(crate) libp2p_webrtc_utils::noise::Error);

impl Error {
    pub(crate) fn from_js_value(value: JsValue) -> Self {
        let s = if value.is_instance_of::<js_sys::Error>() {
            js_sys::Error::from(value)
                .to_string()
                .as_string()
                .unwrap_or_else(|| "Unknown error".to_string())
        } else {
            "Unknown error".to_string()
        };

        Error::Js(s)
    }
}

impl From<JsValue> for Error {
    fn from(value: JsValue) -> Self {
        Error::from_js_value(value)
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Error::Js(value)
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::Js(value.to_string())
    }
}
