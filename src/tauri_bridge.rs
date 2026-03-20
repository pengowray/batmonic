use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Get the Tauri internals object, if running in Tauri.
pub fn get_tauri_internals() -> Option<JsValue> {
    let window = web_sys::window()?;
    let tauri = js_sys::Reflect::get(&window, &JsValue::from_str("__TAURI_INTERNALS__")).ok()?;
    if tauri.is_undefined() {
        None
    } else {
        Some(tauri)
    }
}

/// Invoke a Tauri command and return the result as a JsValue.
pub async fn tauri_invoke(cmd: &str, args: &JsValue) -> Result<JsValue, String> {
    let tauri = get_tauri_internals().ok_or("Not running in Tauri")?;
    let invoke = js_sys::Reflect::get(&tauri, &JsValue::from_str("invoke"))
        .map_err(|_| "No invoke function")?;
    let invoke_fn = js_sys::Function::from(invoke);

    let promise_val = invoke_fn
        .call2(&tauri, &JsValue::from_str(cmd), args)
        .map_err(|e| format!("Invoke call failed: {:?}", e))?;

    let promise: js_sys::Promise = promise_val
        .dyn_into()
        .map_err(|_| "Result is not a Promise")?;

    JsFuture::from(promise)
        .await
        .map_err(|e| format!("Command '{}' failed: {:?}", cmd, e))
}

/// Invoke a Tauri command with no arguments.
pub async fn tauri_invoke_no_args(cmd: &str) -> Result<JsValue, String> {
    tauri_invoke(cmd, &js_sys::Object::new().into()).await
}

/// Read a byte range from a native file via Tauri IPC.
///
/// Returns the raw bytes for the range `[offset, offset + length)`.
pub async fn read_file_range(path: &str, offset: u64, length: u64) -> Result<Vec<u8>, String> {
    let args = js_sys::Object::new();
    js_sys::Reflect::set(&args, &JsValue::from_str("path"), &JsValue::from_str(path))
        .map_err(|e| format!("set path: {:?}", e))?;
    js_sys::Reflect::set(&args, &JsValue::from_str("offset"), &JsValue::from_f64(offset as f64))
        .map_err(|e| format!("set offset: {:?}", e))?;
    js_sys::Reflect::set(&args, &JsValue::from_str("length"), &JsValue::from_f64(length as f64))
        .map_err(|e| format!("set length: {:?}", e))?;

    let result = tauri_invoke("read_file_range", &args.into()).await?;

    // Tauri IPC returns binary data as ArrayBuffer
    let array_buffer = result
        .dyn_into::<js_sys::ArrayBuffer>()
        .map_err(|_| "Expected ArrayBuffer from read_file_range".to_string())?;
    let uint8 = js_sys::Uint8Array::new(&array_buffer);
    Ok(uint8.to_vec())
}

/// Subscribe to a Tauri event. The closure is leaked (via `forget`) so the
/// listener lives for the lifetime of the app.  Returns `true` if the
/// listener was registered successfully.
pub fn tauri_listen(event_name: &str, callback: Closure<dyn FnMut(JsValue)>) -> bool {
    let Some(tauri) = get_tauri_internals() else { return false };

    let Ok(transform_fn) = js_sys::Reflect::get(&tauri, &JsValue::from_str("transformCallback")) else { return false };
    let transform_fn = js_sys::Function::from(transform_fn);
    let Ok(handler_id) = transform_fn.call1(&tauri, callback.as_ref().unchecked_ref()) else { return false };

    let Ok(invoke_fn) = js_sys::Reflect::get(&tauri, &JsValue::from_str("invoke")) else { return false };
    let invoke_fn = js_sys::Function::from(invoke_fn);

    let args = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&args, &"event".into(), &JsValue::from_str(event_name));
    let target = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&target, &"kind".into(), &JsValue::from_str("Any"));
    let _ = js_sys::Reflect::set(&args, &"target".into(), &target);
    let _ = js_sys::Reflect::set(&args, &"handler".into(), &handler_id);

    let _ = invoke_fn.call2(&tauri, &JsValue::from_str("plugin:event|listen"), &args);

    // Leak the closure so it lives forever (event listener for app lifetime)
    callback.forget();

    true
}
