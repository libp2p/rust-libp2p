use js_sys::Promise;
use send_wrapper::SendWrapper;
use wasm_bindgen::{JsCast, JsValue};

use crate::Error;

/// Properly detach a promise.
///
/// A promise always runs in the background, however if you don't await it,
/// or specify a `catch` handler before you drop it, it might cause some side
/// effects. This function avoids any side effects.
//
// Ref: https://github.com/typescript-eslint/typescript-eslint/blob/391a6702c0a9b5b3874a7a27047f2a721f090fb6/packages/eslint-plugin/docs/rules/no-floating-promises.md
pub(crate) fn detach_promise(promise: Promise) {
    type Closure = wasm_bindgen::closure::Closure<dyn FnMut(JsValue)>;
    static mut DO_NOTHING: Option<SendWrapper<Closure>> = None;

    // Allocate Closure only once and reuse it
    let do_nothing = unsafe {
        if DO_NOTHING.is_none() {
            let cb = Closure::new(|_| {});
            DO_NOTHING = Some(SendWrapper::new(cb));
        }

        DO_NOTHING.as_deref().unwrap()
    };

    // Avoid having "floating" promise and ignore any errors.
    // After `catch` promise is allowed to be dropped.
    let _ = promise.catch(do_nothing);
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

pub(crate) fn gen_ufrag(len: usize) -> String {
    let charset = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut ufrag = String::new();
    let mut buf = vec![0; len];
    getrandom::getrandom(&mut buf).unwrap();
    for i in buf {
        let idx = i as usize % charset.len();
        ufrag.push(charset.chars().nth(idx).unwrap());
    }
    ufrag
}
