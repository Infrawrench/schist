//! Browser files have no OS paths. Snapshot the File objects during the drop
//! event, then read them asynchronously into the same store as File > Open.

use futures::channel::mpsc;
use wasm_bindgen::{closure::Closure, JsCast as _};

pub struct FileDropListener {
    window: web_sys::Window,
    handler: Closure<dyn FnMut(web_sys::DragEvent)>,
}

impl Drop for FileDropListener {
    fn drop(&mut self) {
        for kind in ["dragenter", "dragover", "drop"] {
            let _ = self.window.remove_event_listener_with_callback_and_bool(
                kind,
                self.handler.as_ref().unchecked_ref(),
                true,
            );
        }
    }
}

/// The guard unregisters the listeners when the workspace closes. Batches keep
/// their drop order, including two drops that arrive while a file is reading.
pub fn listen_for_file_drops() -> anyhow::Result<(
    FileDropListener,
    mpsc::UnboundedReceiver<Vec<web_sys::File>>,
)> {
    let window = web_sys::window().ok_or_else(|| anyhow::anyhow!("no browser window"))?;
    let (sender, receiver) = mpsc::unbounded();
    let handler =
        Closure::<dyn FnMut(web_sys::DragEvent)>::new(move |event: web_sys::DragEvent| {
            let Some(data) = event.data_transfer() else {
                return;
            };
            // files() is protected until drop. The advertised types remain
            // available during dragover; leave text/link drags alone.
            if !data
                .types()
                .iter()
                .any(|kind| kind.as_string().as_deref() == Some("Files"))
            {
                return;
            }
            event.prevent_default();
            data.set_drop_effect("copy");
            if event.type_() != "drop" {
                return;
            }
            let Some(files) = data.files() else {
                return;
            };
            // The drag data store closes after this callback returns. Retain the
            // File objects now, before awaiting any of their array buffers.
            let files: Vec<_> = (0..files.length()).filter_map(|i| files.item(i)).collect();
            if !files.is_empty() {
                let _ = sender.unbounded_send(files);
            }
        });
    let listener = FileDropListener { window, handler };
    for kind in ["dragenter", "dragover", "drop"] {
        listener
            .window
            .add_event_listener_with_callback_and_bool(
                kind,
                listener.handler.as_ref().unchecked_ref(),
                true,
            )
            .map_err(|err| anyhow::anyhow!("file drop listener: {err:?}"))?;
    }
    Ok((listener, receiver))
}

/// Keep individual failures separate so an unreadable file does not hide the
/// other files in a multi-file drop. The caller localizes the open error.
pub async fn import_dropped_file(file: web_sys::File) -> Result<std::path::PathBuf, String> {
    let name = file.name();
    let buffer = wasm_bindgen_futures::JsFuture::from(file.array_buffer())
        .await
        .map_err(|error| format!("{name}: {error:?}"))?;
    Ok(super::files::store(
        &name,
        js_sys::Uint8Array::new(&buffer).to_vec(),
    ))
}
