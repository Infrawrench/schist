//! Editor file dialogs; binding policy lives in schist-app-actions.
use crate::workspace::Workspace;
#[cfg(not(target_arch = "wasm32"))]
use gpui::PathPromptOptions;
use gpui::{Context, Window};
pub use schist_app_actions::keymap::*;
use schist_i18n::t;
use std::path::PathBuf;

#[cfg(target_arch = "wasm32")]
pub fn open_file_dialog(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    // gpui's web backend has no path prompt to offer (there are no
    // paths); a transient <input type=file> stands in, and the picked
    // bytes land in the in-memory map under an invented path.
    let accept: String = ws
        .registry
        .codecs()
        .flat_map(|c| c.extensions())
        .map(|e| format!(".{e}"))
        .collect::<Vec<_>>()
        .join(",");
    let rx = crate::web::pick_file(&accept);
    cx.spawn_in(window, async move |this, cx| {
        if let Ok(Some(path)) = rx.await {
            this.update_in(cx, |ws, _window, cx| ws.load_file(path, cx))
                .ok();
        }
    })
    .detach();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn open_file_dialog(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let rx = ws.prompt_for_paths(
        PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t("common.open").into()),
        },
        cx,
    );
    cx.spawn_in(window, async move |this, cx| {
        if let Ok(Ok(Some(mut paths))) = rx.await {
            if let Some(path) = paths.pop() {
                this.update_in(cx, |ws, _window, cx| ws.load_file(path, cx))
                    .ok();
            }
        }
    })
    .detach();
}

/// Pick a `.wasm` plugin to install.
#[cfg(not(sandboxed))]
pub fn install_plugin_dialog(
    _ws: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(t("panel.keymap.install_plugin").into()),
    });
    cx.spawn_in(window, async move |this, cx| {
        if let Ok(Ok(Some(mut paths))) = rx.await {
            if let Some(path) = paths.pop() {
                this.update_in(cx, |ws, _window, cx| ws.install_plugin(path, cx))
                    .ok();
            }
        }
    })
    .detach();
}

#[cfg(target_arch = "wasm32")]
pub fn save_file_dialog(ws: &mut Workspace, _window: &mut Window, cx: &mut Context<Workspace>) {
    // No paths to prompt for: the one open question is the file's name
    // (whose extension picks the format), and the browser's own prompt
    // answers it. The save lands as a download.
    let suggested = suggested_name(ws);
    match crate::web::prompt_string(t("panel.keymap.save_as_prompt"), &suggested) {
        Some(name) => {
            let path = PathBuf::from("/web/save").join(name);
            ws.save_file_as(path, cx);
        }
        None => ws.cancel_pending_save(),
    }
}

/// The name a save prompt starts from: the document's stem plus a
/// writable extension, PSD when it has none.
fn suggested_name(ws: &Workspace) -> String {
    ws.doc
        .as_ref()
        .map(|d| {
            let stem = std::path::Path::new(&d.title)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "untitled".into());
            let ext = d
                .path
                .as_ref()
                .and_then(|p| p.extension())
                .and_then(|e| e.to_str())
                .filter(|e| {
                    ws.registry.codecs().any(|codec| {
                        codec.can_export()
                            && codec
                                .extensions()
                                .iter()
                                .any(|ext| ext.eq_ignore_ascii_case(e))
                    })
                })
                .unwrap_or("psd");
            format!("{stem}.{ext}")
        })
        .unwrap_or_else(|| "untitled.psd".into())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save_file_dialog(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let dir = ws
        .doc
        .as_ref()
        .and_then(|d| d.path.as_ref())
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    // PSD is the native save format; keep an existing
    // extension when the document already has a writable one.
    let suggested = suggested_name(ws);
    let rx = ws.prompt_for_new_path(&dir, Some(&suggested), cx);
    cx.spawn_in(window, async move |this, cx| {
        match rx.await {
            Ok(Ok(Some(path))) => {
                this.update_in(cx, |ws, _window, cx| ws.save_file_as(path, cx))
                    .ok();
            }
            // Cancelled, or the prompt failed. Anything waiting on the
            // save -- closing the tab, say -- has to be called off, or it
            // would fire on some later unrelated save instead.
            _ => {
                this.update_in(cx, |ws, _window, _cx| ws.cancel_pending_save())
                    .ok();
            }
        }
    })
    .detach();
}
