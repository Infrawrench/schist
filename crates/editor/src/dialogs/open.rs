//! Dialogs raised while opening a file: what to do with a dropped
//! image, and the HEIC decoder consent prompt.

use super::*;
use schist_i18n::{t, tf, tn};

/// An image was dropped on the window while a document is open: its own
/// tab, or a new layer in the current document?
pub(super) fn drop_image(
    path: std::path::PathBuf,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let tab_path = path.clone();
    ui::modal_frame(
        t("dialog.open.image_title"),
        380.0,
        div()
            .text_size(px(12.0))
            .child(tf!("dialog.open.image_prompt", name = name)),
        div()
            .flex()
            .flex_row()
            .gap_2()
            .child(ui::button(
                t("common.cancel"),
                false,
                |ws, _window, cx| ws.close_modal(cx),
                cx,
            ))
            .child(ui::button(
                t("dialog.open.new_tab"),
                false,
                move |ws, _window, cx| {
                    ws.close_modal(cx);
                    ws.load_file(tab_path.clone(), cx);
                },
                cx,
            ))
            .child(ui::button(
                t("dialog.open.new_layer"),
                true,
                move |ws, _window, cx| {
                    ws.close_modal(cx);
                    ws.place_image_as_layer(path.clone(), cx);
                },
                cx,
            )),
    )
}

/// A HEIC file needs an HEVC decoder this machine doesn't have: ask
/// before downloading one. Consent matters here — it is a network fetch
/// of executable code (hash-pinned to a schist release) and the
/// libraries carry their own (LGPL-3.0) licenses, which are installed
/// alongside.
/// Files another app handed over on iOS: the gallery or the editor?
/// (The desktop never constructs the modal; it opens files outright.)
pub(super) fn shared_image(
    paths: Vec<std::path::PathBuf>,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let name = paths
        .first()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let what = if paths.len() == 1 {
        tf!("dialog.open.quoted", name = name)
    } else {
        tn("dialog.open.these_files", paths.len() as u64)
    };
    let gallery_paths = paths.clone();
    ui::modal_frame(
        t(if paths.len() == 1 {
            "dialog.open.shared_title_one"
        } else {
            "dialog.open.shared_title_many"
        }),
        380.0,
        div()
            .text_size(px(12.0))
            .child(tf!("dialog.open.shared_prompt", what = what)),
        div()
            .flex()
            .flex_row()
            .gap_2()
            .child(ui::button(
                t("common.cancel"),
                false,
                |ws, _window, cx| ws.close_modal(cx),
                cx,
            ))
            .child(ui::button(
                t("dialog.open.add_to_gallery"),
                false,
                move |ws, _window, cx| {
                    ws.close_modal(cx);
                    #[cfg(target_os = "ios")]
                    ws.add_shared_to_gallery(gallery_paths.clone(), cx);
                    #[cfg(not(target_os = "ios"))]
                    let _ = &gallery_paths;
                },
                cx,
            ))
            .child(ui::button(
                t("dialog.open.open_in_editor"),
                true,
                move |ws, _window, cx| {
                    ws.close_modal(cx);
                    #[cfg(target_os = "ios")]
                    ws.open_shared_in_editor(paths.clone(), cx);
                    #[cfg(not(target_os = "ios"))]
                    for path in paths.clone() {
                        ws.load_file(path, cx);
                    }
                },
                cx,
            )),
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn heif_support(
    ws: &Workspace,
    path: std::path::PathBuf,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let managed = schist_codecs_common::heif::managed_library()
        .expect("dialog only opens when a download exists for this platform");
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let downloading = ws.heif_download;
    let source_url = managed.source_url;
    ui::modal_frame(
        t("dialog.open.heic_title"),
        420.0,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(12.0))
            .child(tf!(
                "dialog.open.heic_needs_safe_decoder",
                name = name,
                minimum = schist_codecs_common::heif::MINIMUM_VERSION
            ))
            .child(tf!("dialog.open.heic_offer", version = managed.version)),
        div()
            .flex()
            .flex_row()
            .gap_2()
            .child(ui::button(
                t("common.cancel"),
                false,
                |ws, _window, cx| ws.close_modal(cx),
                cx,
            ))
            .child(ui::button(
                t("dialog.open.licenses_source"),
                false,
                move |_ws, _window, cx| cx.open_url(source_url),
                cx,
            ))
            .child(ui::button(
                if downloading {
                    t("dialog.downloading")
                } else {
                    t("common.download")
                },
                true,
                move |ws, _window, cx| {
                    if !ws.heif_download {
                        ws.close_modal(cx);
                        ws.download_heif_support(path.clone(), cx);
                    }
                },
                cx,
            )),
    )
}

/// Folders dropped on the window: every image in them as tabs, or the
/// folders watched in the gallery. The gallery is the answer for
/// anything bigger than a handful — which is why it is the primary
/// button — and the tab count is capped so a camera roll cannot open
/// five thousand tabs by accident.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn drop_folders(
    dirs: Vec<std::path::PathBuf>,
    images: usize,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let what = if dirs.len() == 1 {
        tf!(
            "dialog.open.quoted",
            name = dirs[0]
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| dirs[0].display().to_string())
        )
    } else {
        tn("dialog.open.n_folders", dirs.len() as u64)
    };
    let cap = crate::workspace::DROP_OPEN_CAP;
    let open_label = if images == 0 {
        t("dialog.open.open_in_tabs").to_string()
    } else if images > cap {
        tf!("dialog.open.open_first_in_tabs", n = cap)
    } else {
        tn("dialog.open.open_n_in_tabs", images as u64)
    };
    let body = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().text_size(px(12.0)).child(tn!(
            "dialog.open.folder_holds",
            images as u64,
            what = what
        )))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(ui::palette().text_dim))
                .child(t("dialog.open.folders_note")),
        );
    let open_dirs = dirs.clone();
    ui::modal_frame(
        t("dialog.open.folders_title"),
        420.0,
        body,
        div()
            .flex()
            .flex_row()
            .gap_2()
            .child(ui::button(
                t("common.cancel"),
                false,
                |ws, _window, cx| ws.close_modal(cx),
                cx,
            ))
            .child(ui::button(
                open_label,
                false,
                move |ws, _window, cx| {
                    ws.close_modal(cx);
                    ws.open_folder_images(open_dirs.clone(), cx);
                },
                cx,
            ))
            .child(ui::button(
                t("dialog.open.add_to_gallery"),
                true,
                move |ws, _window, cx| {
                    ws.close_modal(cx);
                    ws.add_gallery_folders(dirs.clone(), cx);
                },
                cx,
            )),
    )
}
