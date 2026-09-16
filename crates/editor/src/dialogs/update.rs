//! The update-available prompt and its download progress.

use super::*;
use schist_i18n::{t, tf};
use schist_ui::ProgressBar;

/// A newer release than this build.
///
/// Where Schist can replace itself — a macOS bundle, a Windows install —
/// it offers to, and restarts into the new one. Where it cannot, the
/// copy belongs to whatever installed it, so the dialog says where the
/// release is and gets out of the way.
pub(super) fn update_available(
    ws: &Workspace,
    update: crate::update::Update,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let progress = ws.update_progress.clone();
    let page = update.page.clone();
    let installer = update.install.clone();

    let body = div()
        .flex()
        .flex_col()
        .gap_2()
        .text_size(px(12.0))
        .child(tf!(
            "dialog.update.available",
            version = update.version,
            current = crate::update::current_version()
        ));
    let body = match (&installer, &progress) {
        (_, Some(UpdateProgress::Downloading { received, total })) => body
            .child(tf!(
                "dialog.downloading_of",
                got = megabytes(*received),
                total = megabytes(*total)
            ))
            .child(progress_bar(*received as f32 / (*total).max(1) as f32)),
        (_, Some(UpdateProgress::Installing)) => body
            .child(t("dialog.update.installing").to_string())
            .child(progress_bar(1.0)),
        (Some(installer), None) => body.child(tf!(
            "dialog.update.can_install",
            size = megabytes(installer.size)
        )),
        (None, None) => body.child(t("dialog.update.external").to_string()),
    };

    let actions = div().flex().flex_row().gap_2();
    let actions = match progress {
        // Nothing to press while the bundle is being swapped: it takes a
        // moment and there is no half of it to back out to.
        Some(UpdateProgress::Installing) => actions,
        Some(UpdateProgress::Downloading { .. }) => actions.child(ui::button(
            t("common.cancel"),
            false,
            |ws, _window, cx| ws.cancel_update(cx),
            cx,
        )),
        None => {
            let actions = actions.child(ui::button(
                t("dialog.update.later"),
                false,
                |ws, _window, cx| ws.close_modal(cx),
                cx,
            ));
            match installer {
                Some(_) => actions
                    .child(ui::button(
                        t("dialog.update.release_notes"),
                        false,
                        move |_ws, _window, cx| cx.open_url(&page),
                        cx,
                    ))
                    .child(ui::button(
                        t("dialog.update.update_restart"),
                        true,
                        move |ws, _window, cx| ws.start_update(update.clone(), cx),
                        cx,
                    )),
                None => actions.child(ui::button(
                    t("dialog.update.open_page"),
                    true,
                    move |_ws, _window, cx| cx.open_url(&page),
                    cx,
                )),
            }
        }
    };
    ui::modal_frame(t("dialog.update.title"), UPDATE_DIALOG_WIDTH, body, actions)
}

/// The update dialog's width, which its progress bar has to match.
pub(super) const UPDATE_DIALOG_WIDTH: f32 = 420.0;

/// A download's size, in the megabytes a release page would quote.
pub(super) fn megabytes(bytes: u64) -> String {
    tf!(
        "common.megabytes",
        n = format!("{:.1}", bytes as f64 / 1_000_000.0)
    )
}

/// A filled bar, `fraction` of the way across the dialog.
pub(super) fn progress_bar(fraction: f32) -> impl IntoElement {
    // The frame's width less its padding, which is `p_3` on both sides.
    ProgressBar::new(fraction)
        .w(px(UPDATE_DIALOG_WIDTH - 24.0))
        .h(px(6.0))
}
