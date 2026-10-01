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
    let suggested = if ws.design_mode() {
        suggested_layout_name(ws)
    } else {
        suggested_name(ws)
    };
    match crate::web::prompt_string(t("panel.keymap.save_as_prompt"), &suggested) {
        Some(name) => {
            let path = PathBuf::from("/web/save").join(name);
            if ws.design_mode() {
                ws.save_design_as(path, cx);
            } else {
                ws.save_file_as(path, cx);
            }
        }
        None => ws.cancel_pending_save(),
    }
}

/// The name a layout save prompt starts from.
///
/// The document's own stem, with a writable extension. IDML is the only
/// format a layout document can be written as, so that is what it gets
/// when it has none — offering a raster format here would produce a save
/// that cannot land.
fn suggested_layout_name(ws: &Workspace) -> String {
    layout_name(
        ws.design_path.as_deref(),
        ws.design.document.name.trim(),
        |path| ws.design_exporter_for(path).is_some(),
    )
}

/// The naming rule, with the document's own inputs.
///
/// Taken apart from [`suggested_layout_name`] so it can be tested without
/// a window: the rule is three fallbacks and an extension, and all three
/// fallbacks are the sort of thing that breaks silently.
fn layout_name(
    path: Option<&std::path::Path>,
    document_name: &str,
    exporter: impl Fn(&std::path::Path) -> bool,
) -> String {
    let stem = path
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .or_else(|| {
            // A name beginning with a dot is a hidden file, not a document
            // name: `.idml` has no stem in the sense that matters here, and
            // appending an extension to it gives `.idml.idml`. Neither is
            // a name a user could have meant.
            let name = document_name.trim();
            if name.is_empty() || name.starts_with('.') {
                return None;
            }
            Some(
                std::path::Path::new(name)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.to_owned()),
            )
        })
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| "untitled".to_string());

    match path.and_then(|path| path.extension()) {
        // An extension the document already has is kept, so a repeated
        // Save As does not append another one.
        Some(extension) if exporter(path.expect("an extension implies a path")) => {
            format!("{stem}.{}", extension.to_string_lossy())
        }
        // Anything else, including no extension at all, becomes the one
        // format a layout document can be written as.
        _ => format!("{stem}.idml"),
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
    // A layout document's own directory is where it was opened from; the
    // raster path would suggest the photo's, which is a different folder
    // and a confusing thing to save into.
    let design = ws.design_mode();
    let session = ws.design.session.clone();
    let raster = ws.doc.as_ref().map(|d| d.id);
    let dir = match design {
        true => ws
            .design_path
            .as_ref()
            .and_then(|path| path.parent().map(|parent| parent.to_path_buf())),
        false => ws
            .doc
            .as_ref()
            .and_then(|d| d.path.as_ref())
            .and_then(|p| p.parent().map(|p| p.to_path_buf())),
    }
    .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
    .unwrap_or_else(|| PathBuf::from("."));
    // PSD is the native save format for a photo; IDML is the one writable
    // format for a layout document, and an existing extension is kept.
    let suggested = match design {
        true => suggested_layout_name(ws),
        false => suggested_name(ws),
    };
    let rx = ws.prompt_for_new_path(&dir, Some(&suggested), cx);
    cx.spawn_in(window, async move |this, cx| {
        match rx.await {
            Ok(Ok(Some(path))) => {
                this.update_in(cx, |ws, _window, cx| {
                    if design && std::sync::Arc::ptr_eq(&session, &ws.design.session) {
                        ws.save_design_as(path, cx);
                    } else if !design && raster == ws.doc.as_ref().map(|d| d.id) {
                        ws.save_file_as(path, cx);
                    } else {
                        ws.cancel_pending_save();
                        ws.status = schist_i18n::t("design.save_target_changed").into();
                        cx.notify();
                    }
                })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Every path is writable here, which is the common case.
    fn writable(_path: &Path) -> bool {
        true
    }
    /// Nothing is, which is a build where the codec is compiled out.
    fn none(_path: &Path) -> bool {
        false
    }

    #[test]
    fn a_layout_save_suggests_the_idml_the_codec_can_actually_write() {
        // A raster extension here would produce a save that cannot land,
        // so the suggestion is the one writable format.
        assert_eq!(layout_name(None, "brochure", writable), "brochure.idml");
        assert_eq!(layout_name(None, "", writable), "untitled.idml");
    }

    #[test]
    fn a_layout_save_keeps_an_extension_it_can_still_write() {
        // A repeated Save As must not append a second extension.
        assert_eq!(
            layout_name(Some(Path::new("/tmp/brochure.idml")), "brochure", writable),
            "brochure.idml"
        );
    }

    #[test]
    fn a_layout_save_replaces_an_extension_nothing_can_write() {
        // A path left over from a raster document, or a build without the
        // codec, must not be offered as a name that would fail to save.
        assert_eq!(
            layout_name(Some(Path::new("/tmp/photo.psd")), "photo", none),
            "photo.idml"
        );
        assert_eq!(
            layout_name(Some(Path::new("/tmp/photo.psd")), "photo", writable),
            "photo.psd"
        );
    }

    #[test]
    fn a_layout_suggestion_ignores_a_document_name_that_is_only_an_extension() {
        // `.idml` is a file name, not a document name: appending an
        // extension to it gives `.idml.idml`.
        assert_eq!(layout_name(None, ".idml", writable), "untitled.idml");
        assert_eq!(layout_name(None, "..", writable), "untitled.idml");
        assert_eq!(layout_name(None, "  ", writable), "untitled.idml");
    }
}
