use std::io;

use js_sys::{Promise, Reflect};
use once_cell::sync::Lazy;
use send_wrapper::SendWrapper;
use wasm_bindgen::{JsCast, JsValue};

use crate::Error;

type Closure = wasm_bindgen::closure::Closure<dyn FnMut(JsValue)>;
static DO_NOTHING: Lazy<SendWrapper<Closure>> = Lazy::new(|| {
    let cb = Closure::new(|_| {});
    SendWrapper::new(cb)
});

/// Properly detach a promise.
///
/// A promise always runs in the background, however if you don't await it,
/// or specify a `catch` handler before you drop it, it might cause some side
/// effects. This function avoids any side effects.
// Ref: https://github.com/typescript-eslint/typescript-eslint/blob/391a6702c0a9b5b3874a7a27047f2a721f090fb6/packages/eslint-plugin/docs/rules/no-floating-promises.md
pub(crate) fn detach_promise(promise: Promise) {
    // Avoid having "floating" promise and ignore any errors.
    // After `catch` promise is allowed to be dropped.
    let _ = promise.catch(&DO_NOTHING);
}

/// Typecasts a JavaScript type.
///
/// Returns a `Ok(value)` casted to the requested type.
///
/// If the underlying value is an error and the requested
/// type is not, then `Err(Error::JsError)` is returned.
///
/// If the underlying value can not be casted to the requested type and
/// is not an error, then `Err(Error::JsCastFailed)` is returned.
pub(crate) fn to_js_type<T>(value: impl Into<JsValue>) -> Result<T, Error>
where
    T: JsCast + From<JsValue>,
{
    let value = value.into();

    if value.has_type::<T>() {
        Ok(value.unchecked_into())
    } else if value.has_type::<js_sys::Error>() {
        Err(Error::from_js_value(value))
    } else {
        Err(Error::JsCastFailed)
    }
}

/// Parse response from `ReadableStreamDefaultReader::read`.
// Ref: https://streams.spec.whatwg.org/#default-reader-prototype
pub(crate) fn parse_reader_response(resp: &JsValue) -> Result<Option<JsValue>, JsValue> {
    let value = Reflect::get(resp, &JsValue::from_str("value"))?;
    let done = Reflect::get(resp, &JsValue::from_str("done"))?
        .as_bool()
        .unwrap_or_default();

    if value.is_undefined() || done {
        Ok(None)
    } else {
        Ok(Some(value))
    }
}

pub(crate) fn to_io_error(value: JsValue) -> io::Error {
    io::Error::new(io::ErrorKind::Other, Error::from_js_value(value))
}
