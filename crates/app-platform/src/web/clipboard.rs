//! Explicit clipboard reads for dialogs opened by a user gesture.

use wasm_bindgen::JsCast as _;
use wasm_bindgen_futures::JsFuture;

/// Start the request synchronously so browsers can associate it with the
/// gesture. Missing API access or denied permission simply offers no sources.
pub fn read_clipboard_entries() -> impl std::future::Future<Output = Vec<gpui::ClipboardEntry>> {
    let request = web_sys::window().and_then(|window| {
        let clipboard = window.navigator().clipboard();
        let read = js_sys::Reflect::get(&clipboard, &"read".into())
            .ok()?
            .dyn_into::<js_sys::Function>()
            .ok()?;
        read.call0(&clipboard)
            .ok()?
            .dyn_into::<js_sys::Promise>()
            .ok()
    });
    async move {
        let mut entries = Vec::new();
        let Some(request) = request else {
            return entries;
        };
        let Ok(items) = JsFuture::from(request).await else {
            return entries;
        };
        for item in js_sys::Array::from(&items) {
            let Ok(item) = item.dyn_into::<web_sys::ClipboardItem>() else {
                continue;
            };
            for mime in item.types() {
                let Some(mime) = mime.as_string() else {
                    continue;
                };
                let format = gpui::ImageFormat::from_mime_type(&mime);
                if mime != "text/plain" && format.is_none() {
                    continue;
                }
                let Ok(blob) = JsFuture::from(item.get_type(&mime)).await else {
                    continue;
                };
                let Ok(blob) = blob.dyn_into::<web_sys::Blob>() else {
                    continue;
                };
                if let Some(format) = format {
                    if let Ok(buffer) = JsFuture::from(blob.array_buffer()).await {
                        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
                        entries.push(gpui::Image::from_bytes(format, bytes).into());
                    }
                } else if let Ok(text) = JsFuture::from(blob.text()).await {
                    if let Some(text) = text.as_string() {
                        entries.push(text.into());
                    }
                }
            }
        }
        entries
    }
}
