//! Culling controls and two-photo comparison, without changing either original.
use super::gallery_chrome::pal;
use super::*;
use gpui::prelude::FluentBuilder;
use gpui::{img, StatefulInteractiveElement as _};
use image::ImageDecoder as _;
use schist_gallery::culling::{
    self, ColourLabel, CompareCamera, CullEdit, CullFilter, CullFlag, PhotoCulling,
};
use schist_i18n::{t, tf};
use schist_ui::{Button, ButtonColors, Chip, Popover};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static COMPARE_DECODE: std::sync::Mutex<()> = std::sync::Mutex::new(());
const POPUP: Popup = Popup::Field("gallery-culling");

pub(super) struct CompareImage {
    pub render: Arc<RenderImage>,
    pub dimensions: [f32; 2],
    pub preview: bool,
}
pub(super) struct Comparison {
    pub paths: [PathBuf; 2],
    pub images: [Option<CompareImage>; 2],
    pub errors: [Option<String>; 2],
    pub camera: CompareCamera,
    pub active: usize,
    pub areas: [Bounds<Pixels>; 2],
    drag: Option<Point<Pixels>>,
    alive: Arc<AtomicBool>,
}

impl Drop for Comparison {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

impl super::library::Library {
    pub(super) fn culling_of(&self, path: &Path) -> PhotoCulling {
        self.culling.get(path).copied().unwrap_or_default()
    }
}

/// Use full raster detail within a 32-megapixel ceiling; complex formats and
/// larger files use the existing bounded, preview pipeline.
fn decode_comparison(path: &Path) -> anyhow::Result<(u32, u32, Vec<u8>, bool)> {
    let raster = matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" | "tif" | "tiff")
    );
    if raster {
        let decoded = (|| -> anyhow::Result<_> {
            let mut reader = image::ImageReader::open(path)?.with_guessed_format()?;
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(128 * 1024 * 1024);
            reader.limits(limits);
            let mut decoder = reader.into_decoder()?;
            let (w, h) = decoder.dimensions();
            anyhow::ensure!(
                u64::from(w) * u64::from(h) <= 32_000_000 && w <= 8192 && h <= 8192,
                "comparison raster exceeds pixel budget"
            );
            let orientation = decoder.orientation()?;
            let mut image = image::DynamicImage::from_decoder(decoder)?;
            image.apply_orientation(orientation);
            let image = image.into_rgba8();
            Ok((image.width(), image.height(), image.into_raw(), false))
        })();
        if let Ok(decoded) = decoded {
            return Ok(decoded);
        }
    }
    let preview = schist_preview::render_file(path, schist_preview::MAX_EDGE)?;
    Ok((preview.width, preview.height, preview.rgba, true))
}

impl Workspace {
    fn culling_paths(&self) -> Vec<PathBuf> {
        if let Some(compare) = &self.library.comparison {
            return vec![compare.paths[compare.active].clone()];
        }
        if let Some(viewer) = &self.library.viewer {
            return vec![viewer.path.clone()];
        }
        self.library.selected.clone()
    }

    pub(super) fn apply_culling(&mut self, edit: CullEdit, cx: &mut Context<Self>) {
        let paths = self.culling_paths();
        if paths.is_empty() {
            return;
        }
        let previous = self.library.culling.clone();
        culling::edit(&mut self.library.culling, &paths, edit);
        if let Err(error) = self.library.save_checked() {
            self.library.culling = previous;
            self.library.culling_error = Some(tf!("library.ops.save_failed", error = error));
        } else {
            self.library.culling_error = None;
            self.library.culling_changed();
            // Keep decisions on a comparison candidate reachable even when a
            // filter excludes it; in the grid retain only visible selections.
            if self.library.comparison.is_none() && self.library.viewer.is_none() {
                let visible: FxHashSet<_> = self.gallery_flat_order().into_iter().collect();
                self.library.selected.retain(|p| visible.contains(p));
            }
        }
        cx.notify();
    }

    pub(super) fn open_culling_compare(&mut self, cx: &mut Context<Self>) {
        let paths = &self.library.selected;
        if paths.len() != 2 || paths.iter().any(|p| schist_gallery::is_video(p)) {
            return;
        }
        let paths = [paths[0].clone(), paths[1].clone()];
        let sources = paths.each_ref().map(|path| {
            let edited = self.library.entry_of(path).is_some_and(|e| e.edited);
            schist_gallery::thumb_source(path, edited)
        });
        self.close_similar_review();
        self.library.viewer = None;
        self.library.map_view = false;
        self.library.search.active = false;
        self.library.context = None;
        let alive = Arc::new(AtomicBool::new(true));
        self.library.comparison = Some(Comparison {
            paths: paths.clone(),
            images: [None, None],
            errors: [None, None],
            camera: CompareCamera::default(),
            active: 1,
            areas: [Bounds::default(); 2],
            drag: None,
            alive: alive.clone(),
        });
        // Serialize decoders across comparison sessions and skip cancelled work.
        // The session token also rejects stale results when the same pair reopens.
        cx.spawn(async move |this, cx| {
            for (index, source) in sources.into_iter().enumerate() {
                let worker_alive = alive.clone();
                let decoded = cx
                    .background_executor()
                    .spawn(async move {
                        let _guard = COMPARE_DECODE.lock().unwrap_or_else(|e| e.into_inner());
                        worker_alive
                            .load(Ordering::Acquire)
                            .then(|| decode_comparison(&source))
                    })
                    .await;
                let Some(decoded) = decoded else {
                    break;
                };
                let keep_loading = this
                    .update(cx, |ws, cx| {
                        let Some(compare) = ws
                            .library
                            .comparison
                            .as_mut()
                            .filter(|c| Arc::ptr_eq(&c.alive, &alive))
                        else {
                            return false;
                        };
                        match decoded {
                            Ok((w, h, rgba, preview)) => {
                                compare.images[index] = super::library::rgba_to_render_image(
                                    w, h, rgba,
                                )
                                .map(|render| CompareImage {
                                    render,
                                    dimensions: [w as f32, h as f32],
                                    preview,
                                });
                                if compare.images[index].is_none() {
                                    compare.errors[index] = Some(t("common.error").into());
                                }
                            }
                            Err(error) => {
                                compare.errors[index] =
                                    Some(tf!("workspace.docs.open_failed", error = error))
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_loading {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn close_culling_compare(&mut self, cx: &mut Context<Self>) {
        self.library.comparison = None;
        self.culling_filter_changed(cx);
    }

    fn culling_filter_changed(&mut self, cx: &mut Context<Self>) {
        if self.library.comparison.is_none() && self.library.viewer.is_none() {
            let visible: FxHashSet<_> = self.gallery_flat_order().into_iter().collect();
            self.library.selected.retain(|p| visible.contains(p));
        }
        cx.notify();
    }

    pub(super) fn gallery_culling_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.library.search.active
            || self.focused_field.is_some()
            || self.ai.input.active
            || self.ai.model_menu
            || self.spotlight.open
        {
            return false;
        }
        let modifiers = ev.keystroke.modifiers;
        let key = ev.keystroke.key.as_str();
        if modifiers.control
            || modifiers.platform
            || modifiers.alt
            || modifiers.function
            || (modifiers.shift && key != "+")
        {
            return false;
        }
        if key == "c" && self.library.comparison.is_none() {
            self.open_culling_compare(cx);
            return true;
        }
        if let Some(compare) = &mut self.library.comparison {
            match key {
                "escape" | "space" => {
                    self.close_culling_compare(cx);
                    return true;
                }
                "left" => compare.active = 0,
                "right" => compare.active = 1,
                "tab" => compare.active = 1 - compare.active,
                "+" | "=" => compare.camera.zoom_by(1.25),
                "-" => compare.camera.zoom_by(0.8),
                "f" => compare.camera = CompareCamera::default(),
                _ => {
                    if let Some(edit) = shortcut(key) {
                        self.apply_culling(edit, cx);
                        return true;
                    }
                    return false;
                }
            }
            cx.notify();
            return true;
        }
        if let Some(edit) = shortcut(key) {
            self.apply_culling(edit, cx);
            true
        } else {
            false
        }
    }
}

fn shortcut(key: &str) -> Option<CullEdit> {
    Some(match key {
        "0" | "1" | "2" | "3" | "4" | "5" => CullEdit::Rating(key.as_bytes()[0] - b'0'),
        "p" => CullEdit::Flag(CullFlag::Pick),
        "x" => CullEdit::Flag(CullFlag::Reject),
        "u" => CullEdit::Flag(CullFlag::None),
        "6" => CullEdit::Label(ColourLabel::Red),
        "7" => CullEdit::Label(ColourLabel::Yellow),
        "8" => CullEdit::Label(ColourLabel::Green),
        "9" => CullEdit::Label(ColourLabel::Blue),
        "m" => CullEdit::Label(ColourLabel::Magenta),
        "l" => CullEdit::Label(ColourLabel::None),
        _ => return None,
    })
}

fn flag_name(flag: CullFlag) -> &'static str {
    match flag {
        CullFlag::None => t("common.none"),
        CullFlag::Pick => t("culling.pick"),
        CullFlag::Reject => t("culling.reject"),
    }
}
const LABELS: [ColourLabel; 6] = [
    ColourLabel::None,
    ColourLabel::Red,
    ColourLabel::Yellow,
    ColourLabel::Green,
    ColourLabel::Blue,
    ColourLabel::Magenta,
];
fn colour(label: ColourLabel) -> u32 {
    match label {
        ColourLabel::None => pal().text_dim,
        ColourLabel::Red => 0xD84B4B,
        ColourLabel::Yellow => 0xD0AD25,
        ColourLabel::Green => 0x39AB60,
        ColourLabel::Blue => 0x428FD9,
        ColourLabel::Magenta => 0xCB50BD,
    }
}
fn colour_name(label: ColourLabel) -> &'static str {
    match label {
        ColourLabel::None => t("common.none"),
        ColourLabel::Red => t("common.red"),
        ColourLabel::Yellow => t("common.yellow"),
        ColourLabel::Green => t("common.green"),
        ColourLabel::Blue => t("common.blue"),
        ColourLabel::Magenta => t("common.magenta"),
    }
}

pub(super) fn badge(value: PhotoCulling) -> Option<gpui::AnyElement> {
    if value == PhotoCulling::default() {
        return None;
    }
    let flag = match value.flag {
        CullFlag::None => "",
        flag => flag_name(flag),
    };
    Some(
        div()
            .absolute()
            .bottom_1()
            .left_1()
            .rounded_sm()
            .px_2()
            .py_0p5()
            .bg(gpui::rgba(0x000000CC))
            .text_color(gpui::rgb(0xFFFFFF))
            .text_size(px(10.0))
            .border_l_2()
            .border_color(gpui::rgb(colour(value.label)))
            .child(if value.rating == 0 && flag.is_empty() {
                colour_name(value.label).to_string()
            } else {
                format!("{} {flag}", "★".repeat(value.rating as usize))
                    .trim()
                    .to_string()
            })
            .into_any_element(),
    )
}

fn control_group() -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .p_0p5()
        .rounded_sm()
        .bg(gpui::rgb(pal().tray_bg))
}

/// Keep culling controls within reach without taking space from the photo grid.
pub(super) fn toolbar_button(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let open = ws.open_popup == Some(POPUP);
    let filtered = ws.library.culling_filter != CullFilter::default();
    div()
        .relative()
        .flex_none()
        .child(
            Button::new("cull-controls-toggle", t("menu.filter"))
                .colors(ButtonColors {
                    bg: Some(pal().button_bg),
                    hover: pal().button_hover,
                    text: pal().text,
                    border: Some(pal().chrome_edge),
                })
                .rounded_md()
                .active(open || filtered)
                .child(schist_ui::icon(
                    "chevron-down",
                    11.0,
                    if open || filtered {
                        schist_ui::palette().accent_text
                    } else {
                        pal().text
                    },
                ))
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.commit_focused_field();
                    ws.library.search.active = false;
                    ws.gallery_more = None;
                    ws.toggle_popup(POPUP, cx);
                })),
        )
        .children(open.then(|| {
            gpui::deferred(
                div().absolute().left_0().top(px(30.0)).size_0().child(
                    gpui::anchored().snap_to_window_with_margin(px(8.0)).child(
                        Popover::new("cull-controls-popover")
                            .in_flow()
                            .w(px(if ws.gallery_compact { 300.0 } else { 680.0 }))
                            .p_2()
                            .on_dismiss(cx.listener(|ws, _, _, cx| {
                                ws.close_popup(cx);
                                // Consume the outside press, including on the
                                // trigger, so it cannot immediately reopen.
                                cx.stop_propagation();
                            }))
                            .child(controls(ws, cx)),
                    ),
                ),
            )
        }))
        .into_any_element()
}

fn controls(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let paths = ws.culling_paths();
    let values: Vec<_> = paths.iter().map(|p| ws.library.culling_of(p)).collect();
    let disabled = paths.is_empty();
    let comparing = ws.library.comparison.is_some();
    let mut ratings = control_group().child(
        Button::new(("cull-rating", 0usize), "×")
            .ghost()
            .w(px(24.0))
            .px_0()
            .tooltip(t("common.reset"), Some("0".into()))
            .disabled(disabled)
            .on_click(cx.listener(|ws, _, _, cx| ws.apply_culling(CullEdit::Rating(0), cx))),
    );
    for rating in 1..=5 {
        let filled = !disabled && values.iter().all(|v| v.rating >= rating);
        ratings = ratings.child(
            Button::new(
                ("cull-rating", rating as usize),
                if filled { "★" } else { "☆" },
            )
            .ghost()
            .w(px(24.0))
            .px_0()
            .text_size(px(15.0))
            .text_color(gpui::rgb(if filled { pal().text } else { pal().text_dim }))
            .tooltip(format!("{rating} ★"), Some(rating.to_string().into()))
            .disabled(disabled)
            .on_click(
                cx.listener(move |ws, _, _, cx| ws.apply_culling(CullEdit::Rating(rating), cx)),
            ),
        );
    }
    let mut flags = control_group();
    for (index, (flag, key)) in [
        (CullFlag::Pick, "P"),
        (CullFlag::Reject, "X"),
        (CullFlag::None, "U"),
    ]
    .into_iter()
    .enumerate()
    {
        flags = flags.child(
            Button::new(("cull-flag", index), flag_name(flag))
                .ghost()
                .px_2()
                .tooltip(flag_name(flag), Some(key.into()))
                .active(!disabled && values.iter().all(|v| v.flag == flag))
                .disabled(disabled)
                .on_click(
                    cx.listener(move |ws, _, _, cx| ws.apply_culling(CullEdit::Flag(flag), cx)),
                ),
        );
    }
    let mut labels = control_group();
    for (index, label) in LABELS.into_iter().enumerate() {
        let key = ["L", "6", "7", "8", "9", "M"][index];
        labels = labels.child(
            Button::bare(("cull-label", index))
                .ghost()
                .w(px(24.0))
                .px_0()
                .child(label_dot(label))
                .tooltip(colour_name(label), Some(key.into()))
                .active(!disabled && values.iter().all(|v| v.label == label))
                .disabled(disabled)
                .on_click(
                    cx.listener(move |ws, _, _, cx| ws.apply_culling(CullEdit::Label(label), cx)),
                ),
        );
    }
    let mut edits = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            div()
                .min_w(px(60.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(t(if comparing {
                    "common.photo"
                } else {
                    "common.selection"
                })),
        )
        .child(ratings)
        .child(flags)
        .child(labels);
    if !comparing {
        edits = edits.child(div().flex_grow()).child(
            Button::new("cull-compare", t("culling.compare"))
                .px_2()
                .tooltip(t("culling.compare"), Some("C".into()))
                .disabled(
                    ws.library.selected.len() != 2
                        || ws
                            .library
                            .selected
                            .iter()
                            .any(|p| schist_gallery::is_video(p)),
                )
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.close_popup(cx);
                    ws.open_culling_compare(cx);
                })),
        );
    }
    let filter = ws.library.culling_filter;
    let mut minimums = control_group();
    for rating in 1..=5 {
        minimums = minimums.child(
            Chip::new(("cull-min", rating as usize), format!("{rating}★+"))
                .rounded_sm()
                .px_2()
                .selected(filter.minimum_rating == rating)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.library.culling_filter.minimum_rating =
                        if ws.library.culling_filter.minimum_rating == rating {
                            0
                        } else {
                            rating
                        };
                    ws.culling_filter_changed(cx);
                })),
        );
    }
    let mut filter_flags = control_group();
    for (index, flag) in [CullFlag::Pick, CullFlag::Reject, CullFlag::None]
        .into_iter()
        .enumerate()
    {
        filter_flags = filter_flags.child(
            Chip::new(("cull-filter-flag", index), flag_name(flag))
                .rounded_sm()
                .px_2()
                .selected(filter.flag == Some(flag))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.library.culling_filter.flag = if ws.library.culling_filter.flag == Some(flag)
                    {
                        None
                    } else {
                        Some(flag)
                    };
                    ws.culling_filter_changed(cx);
                })),
        );
    }
    let mut filter_labels = control_group();
    for (index, label) in LABELS.into_iter().enumerate() {
        filter_labels = filter_labels.child(
            Chip::new(("cull-filter-label", index), "")
                .w(px(24.0))
                .px_0()
                .rounded_sm()
                .child(label_dot(label))
                .tooltip(colour_name(label), None)
                .selected(filter.label == Some(label))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.library.culling_filter.label =
                        if ws.library.culling_filter.label == Some(label) {
                            None
                        } else {
                            Some(label)
                        };
                    ws.culling_filter_changed(cx);
                })),
        );
    }
    let filters = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(
            div()
                .min_w(px(60.0))
                .text_color(gpui::rgb(pal().text_dim))
                .child(t("menu.filter")),
        )
        .child(
            Chip::new("cull-all", t("common.all"))
                .rounded_sm()
                .selected(filter == CullFilter::default())
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.library.culling_filter = CullFilter::default();
                    ws.culling_filter_changed(cx);
                })),
        )
        .child(minimums)
        .child(filter_flags)
        .child(filter_labels);
    div()
        .id("cull-controls")
        .flex()
        .flex_col()
        .max_h(px((ws.visible_height - 100.0).max(120.0)))
        .overflow_y_scroll()
        .gap_2()
        .text_size(px(11.0))
        .child(edits)
        .when(!comparing, |bar| bar.child(filters))
        .into_any_element()
}

fn label_dot(label: ColourLabel) -> gpui::AnyElement {
    if label == ColourLabel::None {
        div().text_size(px(13.0)).child("×").into_any_element()
    } else {
        div()
            .size(px(9.0))
            .rounded_full()
            .bg(gpui::rgb(colour(label)))
            .into_any_element()
    }
}

pub(super) fn comparison(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let compare = ws.library.comparison.as_ref().unwrap();
    let zoom = compare.camera.zoom;
    let header = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_2()
        .items_center()
        .px_2()
        .py_1()
        .bg(gpui::rgb(pal().chrome_bg))
        .border_b_1()
        .border_color(gpui::rgb(pal().chrome_edge))
        .child(
            Button::new("compare-close", t("common.close"))
                .ghost()
                .px_2()
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.close_culling_compare(cx);
                })),
        )
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(t("culling.compare")),
        )
        .child(div().flex_grow())
        .child(
            Button::new("compare-fit", t("menu.view.fit_on_screen"))
                .ghost()
                .px_2()
                .on_click(cx.listener(|ws, _, _, cx| {
                    if let Some(c) = &mut ws.library.comparison {
                        c.camera = CompareCamera::default();
                    }
                    cx.notify();
                })),
        )
        .child(
            Button::new("compare-actual", t("menu.view.actual_size"))
                .ghost()
                .px_2()
                .on_click(cx.listener(|ws, _, _, cx| {
                    if let Some(c) = &mut ws.library.comparison {
                        if let Some(image) = &c.images[c.active] {
                            let area = c.areas[c.active].size;
                            let fit = (f32::from(area.width) / image.dimensions[0])
                                .min(f32::from(area.height) / image.dimensions[1])
                                .min(1.0);
                            if fit > 0.0 {
                                c.camera.zoom = (1.0 / fit).clamp(1.0, 32.0);
                            }
                        }
                    }
                    cx.notify();
                })),
        )
        .child(
            Button::new("compare-out", "−")
                .ghost()
                .w(px(24.0))
                .px_0()
                .on_click(cx.listener(|ws, _, _, cx| {
                    if let Some(c) = &mut ws.library.comparison {
                        c.camera.zoom_by(0.8);
                    }
                    cx.notify();
                })),
        )
        .child(
            div()
                .min_w(px(40.0))
                .text_center()
                .text_color(gpui::rgb(pal().text_dim))
                .child(format!("{zoom:.2}×")),
        )
        .child(
            Button::new("compare-in", "+")
                .ghost()
                .w(px(24.0))
                .px_0()
                .on_click(cx.listener(|ws, _, _, cx| {
                    if let Some(c) = &mut ws.library.comparison {
                        c.camera.zoom_by(1.25);
                    }
                    cx.notify();
                })),
        );
    let mut panes = div()
        .flex()
        .flex_row()
        .p_2()
        .gap_2()
        .flex_grow()
        .min_h(px(0.0))
        .min_w(px(0.0));
    for index in 0..2 {
        panes = panes.child(compare_pane(ws, index, cx));
    }
    div()
        .flex()
        .flex_col()
        .flex_grow()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .child(header)
        .child(panes)
        .into_any_element()
}

fn compare_pane(ws: &Workspace, index: usize, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let c = ws.library.comparison.as_ref().unwrap();
    let path = c.paths[index].clone();
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let active = c.active == index;
    let entity = cx.entity();
    let mut picture = div()
        .id(("compare-picture", index))
        .relative()
        .flex_grow()
        .min_h(px(0.0))
        .overflow_hidden()
        .bg(gpui::rgb(0x161616))
        .cursor(gpui::CursorStyle::OpenHand)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, ev: &MouseDownEvent, _, cx| {
                if let Some(c) = &mut ws.library.comparison {
                    c.active = index;
                    c.drag = Some(ev.position);
                }
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |ws, ev: &MouseMoveEvent, _, cx| {
            let Some(c) = &mut ws.library.comparison else {
                return;
            };
            if ev.pressed_button != Some(MouseButton::Left) {
                c.drag = None;
                return;
            }
            let Some(previous) = c.drag else {
                return;
            };
            c.drag = Some(ev.position);
            let Some(image) = &c.images[c.active] else {
                return;
            };
            let area = c.areas[c.active].size;
            let rect = c
                .camera
                .image_rect(image.dimensions, [area.width.into(), area.height.into()]);
            c.camera.pan(
                [
                    (ev.position.x - previous.x).into(),
                    (ev.position.y - previous.y).into(),
                ],
                [rect[2], rect[3]],
            );
            cx.notify();
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|ws, _: &MouseUpEvent, _, _| {
                if let Some(c) = &mut ws.library.comparison {
                    c.drag = None;
                }
            }),
        )
        .on_scroll_wheel(cx.listener(|ws, ev: &gpui::ScrollWheelEvent, _, cx| {
            let dy = match ev.delta {
                gpui::ScrollDelta::Pixels(p) => f32::from(p.y),
                gpui::ScrollDelta::Lines(l) => l.y * 40.0,
            };
            if let Some(c) = &mut ws.library.comparison {
                c.camera.zoom_by((dy * 0.008).exp());
            }
            cx.notify();
            cx.stop_propagation();
        }))
        .child(
            canvas(
                move |bounds, _, cx| {
                    entity.update(cx, |ws, cx| {
                        if let Some(c) = &mut ws.library.comparison {
                            if c.areas[index] != bounds {
                                c.areas[index] = bounds;
                                cx.notify();
                            }
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );
    if let Some(image) = &c.images[index] {
        let area = c.areas[index].size;
        let [x, y, w, h] = c
            .camera
            .image_rect(image.dimensions, [area.width.into(), area.height.into()]);
        picture = picture.child(
            img(image.render.clone())
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h)),
        );
        if image.preview {
            picture = picture.child(
                div()
                    .absolute()
                    .top_1()
                    .left_1()
                    .text_color(gpui::rgb(0xFFFFFF))
                    .child(t("common.preview")),
            );
        }
    } else {
        picture = picture.child(
            div().p_3().text_color(gpui::rgb(0xFFFFFF)).child(
                c.errors[index]
                    .clone()
                    .unwrap_or_else(|| t("common.loading").into()),
            ),
        );
    }
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .rounded_sm()
        .overflow_hidden()
        .border_1()
        .border_color(gpui::rgb(if active {
            pal().select_border
        } else {
            pal().chrome_edge
        }))
        .child(
            Button::bare(("compare-name", index))
                .ghost()
                .h(px(28.0))
                .w_full()
                .min_w(px(0.0))
                .px_2()
                .justify_start()
                .bg(gpui::rgb(if active {
                    pal().select_fill
                } else {
                    pal().chrome_bg
                }))
                .tooltip(path.display().to_string(), None)
                .child(
                    div()
                        .flex_none()
                        .text_color(gpui::rgb(pal().text_dim))
                        .child((index + 1).to_string()),
                )
                .child(div().flex_1().min_w(px(0.0)).truncate().child(name))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    if let Some(c) = &mut ws.library.comparison {
                        c.active = index;
                    }
                    cx.notify();
                })),
        )
        .child(picture.children(badge(ws.library.culling_of(&path))))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn culling_shortcuts_distinguish_ratings_flags_and_labels() {
        assert_eq!(shortcut("5"), Some(CullEdit::Rating(5)));
        assert_eq!(shortcut("x"), Some(CullEdit::Flag(CullFlag::Reject)));
        assert_eq!(shortcut("u"), Some(CullEdit::Flag(CullFlag::None)));
        assert_eq!(shortcut("9"), Some(CullEdit::Label(ColourLabel::Blue)));
        assert_eq!(shortcut("l"), Some(CullEdit::Label(ColourLabel::None)));
        assert_eq!(shortcut("50"), None);
    }
    #[test]
    fn comparison_respects_camera_orientation_and_reports_decode_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("portrait.jpg");
        let source = image::RgbImage::from_pixel(6, 3, image::Rgb([13, 29, 91]));
        source.save(&path).unwrap();
        let jpeg = std::fs::read(&path).unwrap();
        // APP1 Exif: little-endian TIFF with one SHORT Orientation=6 entry.
        let exif = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
        let mut rotated = jpeg[..2].to_vec();
        rotated.extend_from_slice(&[0xff, 0xe1]);
        rotated.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        rotated.extend_from_slice(exif);
        rotated.extend_from_slice(&jpeg[2..]);
        std::fs::write(&path, &rotated).unwrap();
        let (w, h, _, preview) = decode_comparison(&path).unwrap();
        assert_eq!((w, h), (3, 6));
        assert!(!preview);
        assert_eq!(std::fs::read(&path).unwrap(), rotated);
        std::fs::write(&path, b"broken").unwrap();
        assert!(decode_comparison(&path).is_err());
    }

    #[test]
    fn comparison_retains_raster_detail_and_never_writes_originals() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large.png");
        let original = image::RgbaImage::from_pixel(2400, 4, image::Rgba([13, 29, 91, 255]));
        original.save(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let (width, height, rgba, preview) = decode_comparison(&path).unwrap();
        assert_eq!((width, height), (2400, 4));
        assert_eq!(&rgba[..4], &[13, 29, 91, 255]);
        assert!(!preview);
        assert_eq!(bytes, std::fs::read(&path).unwrap());
    }
}
