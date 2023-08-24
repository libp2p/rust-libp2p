use wasm_bindgen::{JsCast, JsValue};

/// Errors that may happen on the [`Transport`](crate::Transport) or the
/// [`Connection`](crate::Connection).
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid multiaddr: {0}")]
    InvalidMultiaddr(&'static str),

    #[error("JavaScript error: {0}")]
    JsError(String),

    #[error("JavaScript typecasting failed")]
    JsCastFailed,

    #[error("Unknown remote peer ID")]
    UnknownRemotePeerId,

    #[error("Connection error: {0}")]
    Connection(String),
}

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

        Error::JsError(s)
    }
}

/// Ensure the libp2p_webrtc_utils::Error is converted to the WebRTC error so we don't expose it to the user
/// via our public API.
impl From<libp2p_webrtc_utils::Error> for Error {
    fn from(e: libp2p_webrtc_utils::Error) -> Self {
        match e {
            libp2p_webrtc_utils::Error::Io(e) => Error::JsError(e.to_string()),
            libp2p_webrtc_utils::Error::Authentication(e) => Error::JsError(e.to_string()),
            libp2p_webrtc_utils::Error::InvalidPeerID { expected, got } => Error::JsError(format!(
                "Invalid peer ID (expected {}, got {})",
                expected, got
            )),
            libp2p_webrtc_utils::Error::NoListeners => Error::JsError("No listeners".to_string()),
            libp2p_webrtc_utils::Error::Internal(e) => Error::JsError(e),
        }
    }
}

impl std::convert::From<wasm_bindgen::JsValue> for Error {
    fn from(value: JsValue) -> Self {
        Error::from_js_value(value)
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Error::JsError(value)
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::JsError(value.to_string())
    }
}
