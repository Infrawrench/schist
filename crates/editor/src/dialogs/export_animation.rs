//! File ▸ Export Animation: the frame animation as GIF, APNG or WebP.

use super::*;
use schist_animation::{Disposal, ExportOptions, Format, PaletteMode};

fn update(ws: &mut Workspace, f: impl FnOnce(&mut ExportOptions)) {
    ws.update_modal(|m| {
        if let Modal::ExportAnimation { options } = m {
            f(options);
        }
    });
}

pub(super) fn export_animation_dialog(
    ws: &mut Workspace,
    state: &DialogState,
    options: ExportOptions,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let field = |id: &'static str| Popup::Field(id);
    let open = |id: &'static str| state.open_popup == Some(Popup::Field(id));
    let mut body = div().flex().flex_col().gap_1().child(ui::field_row(
        t("dialog.export.format"),
        ui::dropdown(
            &ws.dropdown,
            ui::Dropdown {
                popup: field("anim-format"),
                is_open: open("anim-format"),
                current: options.format,
                label: options.format.name().into(),
                width: 170.0,
                options: Format::ALL
                    .iter()
                    .map(|f| (SharedString::from(f.name()), *f))
                    .collect(),
            },
            |ws, format, _cx| update(ws, |o| o.format = format),
            cx,
        ),
    ));
    if options.format == Format::Gif {
        let palettes = [
            (
                t("animation.export.palette_per_frame"),
                PaletteMode::PerFrame,
            ),
            (t("animation.export.palette_global"), PaletteMode::Global),
        ];
        let label = palettes
            .iter()
            .find(|(_, p)| *p == options.gif.palette)
            .map_or("", |(l, _)| l);
        body = body.child(ui::field_row(
            t("animation.export.palette"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: field("anim-palette"),
                    is_open: open("anim-palette"),
                    current: options.gif.palette,
                    label: label.into(),
                    width: 170.0,
                    options: palettes
                        .iter()
                        .map(|(l, p)| (SharedString::from(*l), *p))
                        .collect(),
                },
                |ws, palette, _cx| update(ws, |o| o.gif.palette = palette),
                cx,
            ),
        ));
        let disposals = [
            (t("animation.export.disposal_auto"), Disposal::Auto),
            (t("animation.export.disposal_keep"), Disposal::Keep),
            (
                t("animation.export.disposal_background"),
                Disposal::Background,
            ),
            (t("animation.export.disposal_previous"), Disposal::Previous),
        ];
        let label = disposals
            .iter()
            .find(|(_, d)| *d == options.gif.disposal)
            .map_or("", |(l, _)| l);
        body = body.child(ui::field_row(
            t("animation.export.disposal"),
            ui::dropdown(
                &ws.dropdown,
                ui::Dropdown {
                    popup: field("anim-disposal"),
                    is_open: open("anim-disposal"),
                    current: options.gif.disposal,
                    label: label.into(),
                    width: 170.0,
                    options: disposals
                        .iter()
                        .map(|(l, d)| (SharedString::from(*l), *d))
                        .collect(),
                },
                |ws, disposal, _cx| update(ws, |o| o.gif.disposal = disposal),
                cx,
            ),
        ));
        body = body.child(ui::field_row(
            t("dialog.export.dither"),
            ui::checkbox(
                t("animation.export.dither_note"),
                options.gif.dither,
                |ws, _cx| update(ws, |o| o.gif.dither = !o.gif.dither),
                cx,
            ),
        ));
    }
    body = body.child(ui::field_row(
        t("animation.export.transparency"),
        ui::checkbox(
            t("animation.export.transparency_note"),
            options.transparency,
            |ws, _cx| update(ws, |o| o.transparency = !o.transparency),
            cx,
        ),
    ));
    let note = match options.format {
        Format::Gif => t("animation.export.gif_note"),
        Format::Apng => t("animation.export.apng_note"),
        Format::WebP => t("animation.export.webp_note"),
    };
    body = body.child(
        div()
            .text_size(px(11.0))
            .text_color(gpui::rgb(ui::palette().text_dim))
            .child(note),
    );
    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(ui::button(
            t("common.cancel"),
            false,
            |ws, _w, cx| ws.close_modal(cx),
            cx,
        ))
        .child(ui::button(
            t("dialog.export.export_ellipsis"),
            true,
            move |ws, window, cx| {
                ws.close_modal(cx);
                ws.export_animation(options, window, cx);
            },
            cx,
        ));
    ui::modal_frame(t("animation.export.title"), 380.0, body, actions)
}
