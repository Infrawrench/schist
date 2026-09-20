//! The Info tab: what the open photo's EXIF says, and where it was
//! taken.

use super::*;
use crate::workspace::SideTab;
use schist_gallery::ExifSummary;
use schist_i18n::{t, tf};

/// The tallest the EXIF rows get before they scroll: about three rows.
const INFO_ROWS_MAX_H: f32 = 88.0;
/// The side panel is 260 px wide with 8 px of padding each side; the
/// map spans that.
#[cfg(not(target_arch = "wasm32"))]
const SIDE_PANEL_CONTENT_W: f32 = 260.0 - 16.0;

/// The top dock: Character while using Type, Info for files with EXIF,
/// and Color. Type's detailed controls stay here instead of wrapping the
/// options bar onto the canvas.
pub(super) fn top_panel(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    ws.refresh_exif();
    let exif = ws.exif.as_ref().and_then(|(_, e)| e.clone());
    let is_type = ws.editor.active_tool == "type";
    if exif.is_none() && !is_type {
        return color_panel(ws, cx).into_any_element();
    }
    let default = if is_type {
        SideTab::Character
    } else {
        SideTab::Info
    };
    let tab = match ws.side_tab.unwrap_or(default) {
        SideTab::Character if !is_type => default,
        SideTab::Info if exif.is_none() => SideTab::Color,
        tab => tab,
    };
    let tab_chip =
        |id: &'static str, label: &'static str, which: SideTab, cx: &mut Context<Workspace>| {
            let on = tab == which;
            Chip::new(id, label)
                .selected(on)
                .on_click(cx.listener(move |ws, _e, _w, cx| {
                    ws.commit_focused_field();
                    ws.side_tab = Some(which);
                    cx.notify();
                }))
        };
    let tabs = div()
        .flex()
        .flex_row()
        .gap_1()
        .px_2()
        .pt_2()
        .children(is_type.then(|| {
            tab_chip(
                "side-tab-character",
                t("panel.character.title"),
                SideTab::Character,
                cx,
            )
        }))
        .children(
            exif.is_some()
                .then(|| tab_chip("side-tab-info", t("panel.info.title"), SideTab::Info, cx)),
        )
        .child(tab_chip(
            "side-tab-color",
            t("common.color"),
            SideTab::Color,
            cx,
        ));
    let body = match tab {
        SideTab::Info => info_panel(ws, exif.as_ref().unwrap(), cx).into_any_element(),
        SideTab::Color => color_panel(ws, cx).into_any_element(),
        SideTab::Character => super::typography::character_panel(ws, cx),
    };
    div()
        .flex()
        .flex_col()
        .child(tabs)
        .child(body)
        .into_any_element()
}

pub(super) fn top_panel_label(ws: &Workspace) -> &'static str {
    let has_exif = ws.exif.as_ref().is_some_and(|(_, exif)| exif.is_some());
    let is_type = ws.editor.active_tool == "type";
    if !has_exif && !is_type {
        return t("common.color");
    }
    let default = if is_type {
        SideTab::Character
    } else {
        SideTab::Info
    };
    match ws.side_tab.unwrap_or(default) {
        SideTab::Character if is_type => t("panel.character.title"),
        SideTab::Info if has_exif => t("panel.info.title"),
        _ => t("common.color"),
    }
}

/// The thumb beside the EXIF rows, so it shows there is more below the
/// fold; it reads the scroll handle's own extents from the last frame
/// and is absent while everything fits.
fn rows_thumb(handle: &gpui::ScrollHandle) -> Option<gpui::AnyElement> {
    let view_h = f32::from(handle.bounds().size.height);
    let max_y = f32::from(handle.max_offset().height);
    if view_h <= 0.0 || max_y <= 1.0 {
        return None;
    }
    let thumb_h = (view_h * view_h / (view_h + max_y)).clamp(20.0, view_h);
    let travel = (view_h - thumb_h).max(1.0);
    let scroll_y = (-f32::from(handle.offset().y)).clamp(0.0, max_y);
    let thumb_top = scroll_y / max_y * travel;
    Some(
        div()
            .absolute()
            .top(px(thumb_top))
            .right_0()
            .w(px(3.0))
            .h(px(thumb_h))
            .rounded_sm()
            .bg(gpui::rgb(palette().text_dim))
            .opacity(0.5)
            .into_any_element(),
    )
}

/// The camera, the exposure, when, and — on a map with a blip — where.
fn info_panel(
    ws: &mut Workspace,
    exif: &ExifSummary,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let row = |label: &'static str, value: String| {
        div()
            .flex()
            .flex_row()
            .items_baseline()
            .gap_2()
            .child(
                div()
                    .w(px(64.0))
                    .flex_none()
                    .text_size(px(10.0))
                    .text_color(gpui::rgb(palette().text_dim))
                    .child(label),
            )
            .child(
                div()
                    .flex_grow()
                    .min_w(px(0.0))
                    .text_size(px(11.0))
                    .text_color(gpui::rgb(palette().text))
                    .child(SharedString::from(value)),
            )
    };
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    if let Some(camera) = exif.camera() {
        rows.push(row(t("panel.info.camera"), camera).into_any_element());
    }
    if let Some(lens) = &exif.lens {
        rows.push(row(t("panel.info.lens"), lens.clone()).into_any_element());
    }
    // The exposure triangle on one line, as a camera's own display
    // puts it.
    let exposure: Vec<String> = [
        exif.exposure.clone(),
        exif.aperture.clone(),
        exif.iso.map(|iso| tf!("panel.info.iso", iso = iso)),
        exif.focal_length.clone(),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !exposure.is_empty() {
        rows.push(row(t("common.exposure"), exposure.join(" \u{b7} ")).into_any_element());
    }
    if let Some(bias) = &exif.exposure_bias {
        rows.push(row(t("panel.info.bias"), bias.clone()).into_any_element());
    }
    let mut extras: Vec<String> = Vec::new();
    if let Some(flash) = exif.flash {
        extras.push(if flash {
            t("panel.info.flash_fired").into()
        } else {
            t("panel.info.no_flash").into()
        });
    }
    if let Some(wb) = &exif.white_balance {
        extras.push(tf!("panel.info.white_balance", wb = wb.to_lowercase()));
    }
    if let Some(metering) = &exif.metering {
        extras.push(tf!(
            "panel.info.metering",
            metering = metering.to_lowercase()
        ));
    }
    if !extras.is_empty() {
        rows.push(row(t("common.settings"), extras.join(", ")).into_any_element());
    }
    if let Some(taken) = &exif.taken {
        rows.push(row(t("panel.info.taken"), taken.clone()).into_any_element());
    }
    if let (Some(w), Some(h)) = (exif.width, exif.height) {
        let mp = format!("{:.1}", (w as f64 * h as f64) / 1_000_000.0);
        let size = match exif.orientation {
            Some(o) if o > 1 => tf!("panel.info.size_oriented", w = w, h = h, mp = mp, o = o),
            _ => tf!("panel.info.size", w = w, h = h, mp = mp),
        };
        rows.push(row(t("common.size"), size).into_any_element());
    }
    if let Some(software) = &exif.software {
        rows.push(row(t("panel.info.software"), software.clone()).into_any_element());
    }
    if let Some((lat, lon)) = exif.gps {
        let place = schist_gallery::nearest_city(lat, lon);
        let mut text = match place {
            Some(place) => format!("{place} \u{b7} {lat:.4}, {lon:.4}"),
            None => format!("{lat:.4}, {lon:.4}"),
        };
        if let Some(alt) = exif.altitude_m {
            text.push_str(&format!(" \u{b7} {alt:.0} m"));
        }
        rows.push(row(t("panel.info.where"), text).into_any_element());
    }
    // The rows are their own scrolling region, bounded so the map
    // beneath stays put whatever a camera wrote (some write a lot).
    let panel = div().flex().flex_col().p_2().gap_1().child(
        div()
            .relative()
            .child(
                div()
                    .id("info-rows")
                    .flex()
                    .flex_col()
                    .gap_1()
                    .max_h(px(INFO_ROWS_MAX_H))
                    .overflow_y_scroll()
                    .track_scroll(&ws.info_scroll)
                    .children(rows),
            )
            .children(rows_thumb(&ws.info_scroll)),
    );
    #[cfg(not(target_arch = "wasm32"))]
    let panel = if exif.gps.is_some() {
        // The map, with the blip on it, at 16:9 across the panel's
        // content width. Wheel to zoom, drag to pan; it opens on the
        // spot at street scale.
        let map_h = (SIDE_PANEL_CONTENT_W * 9.0 / 16.0).round();
        panel.child(div().pt_1().child(crate::workspace::map_element(
            ws,
            crate::workspace::MapSlot::Info,
            map_h,
            cx,
        )))
    } else {
        panel
    };
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (ws, cx);
    }
    panel
}
