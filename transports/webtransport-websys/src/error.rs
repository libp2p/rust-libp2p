use wasm_bindgen::{JsCast, JsValue};

/// Errors that may happen on the [`Transport`](crate::Transport) or the
/// [`Connection`](crate::Connection).
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid multiaddr: {0}")]
    InvalidMultiaddr(&'static str),

    #[error("Noise authentication failed")]
    Noise(#[from] libp2p_noise::Error),

    #[error("JavaScript error: {0}")]
    #[allow(clippy::enum_variant_names)]
    JsError(String),

    #[error("JavaScript typecasting failed")]
    JsCastFailed,

    #[error("Unknown remote peer ID")]
    UnknownRemotePeerId,
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
