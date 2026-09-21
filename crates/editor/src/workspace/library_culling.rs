//! Culling controls and two-photo comparison, without changing either original.
use super::*;
use image::ImageDecoder as _;
use schist_gallery::culling::{self, CompareCamera, CullEdit, CullFilter, PhotoCulling};
use schist_gallery_ui::comparison::{
    self as compare_ui, CompareAction, ComparisonActions, ComparisonPane, ComparisonToolbar,
};
use schist_i18n::{t, tf};
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

pub(super) use schist_gallery_ui::culling::badge;
use schist_gallery_ui::culling::{
    self as controls_ui, shortcut, CullingActions, CullingControls, CullingPopover,
};

pub(super) fn toolbar_button(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let open = ws.open_popup == Some(POPUP);
    let content = open.then(|| {
        let values = ws
            .culling_paths()
            .iter()
            .map(|p| ws.library.culling_of(p))
            .collect();
        controls_ui::controls(
            CullingControls {
                values,
                busy: false,
                comparing: ws.library.comparison.is_some(),
                can_compare: ws.library.selected.len() == 2
                    && !ws
                        .library
                        .selected
                        .iter()
                        .any(|p| schist_gallery::is_video(p)),
                filter: ws.library.culling_filter,
                max_height: (ws.visible_height - 100.0).max(120.0),
            },
            CullingActions {
                edit: Workspace::apply_culling,
                filter: |ws, action, cx| {
                    ws.library.culling_filter = action.apply(ws.library.culling_filter);
                    ws.culling_filter_changed(cx);
                },
                compare: |ws, cx| {
                    ws.close_popup(cx);
                    ws.open_culling_compare(cx);
                },
            },
            cx,
        )
    });
    controls_ui::toolbar(
        CullingPopover {
            open,
            filtered: ws.library.culling_filter != CullFilter::default(),
            compact: ws.gallery_compact,
        },
        content,
        |ws, cx| {
            ws.commit_focused_field();
            ws.library.search.active = false;
            ws.gallery_more = None;
            ws.toggle_popup(POPUP, cx);
        },
        |ws, cx| ws.close_popup(cx),
        cx,
    )
}

pub(super) fn comparison(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let compare = ws.library.comparison.as_ref().unwrap();
    let header = compare_ui::toolbar(
        ComparisonToolbar {
            title: t("culling.compare"),
            zoom: compare.camera.zoom,
            actual_size_enabled: compare.images[compare.active].is_some(),
        },
        |ws, action, cx| {
            if matches!(action, CompareAction::Close) {
                ws.close_culling_compare(cx);
                return;
            }
            if let Some(c) = &mut ws.library.comparison {
                match action {
                    CompareAction::Fit => c.camera = CompareCamera::default(),
                    CompareAction::ActualSize => {
                        if let Some(image) = &c.images[c.active] {
                            c.camera.actual_size(
                                image.dimensions,
                                [
                                    c.areas[c.active].size.width.into(),
                                    c.areas[c.active].size.height.into(),
                                ],
                            );
                        }
                    }
                    CompareAction::ZoomOut => c.camera.zoom_by(0.8),
                    CompareAction::ZoomIn => c.camera.zoom_by(1.25),
                    CompareAction::Close => unreachable!(),
                }
            }
            cx.notify();
        },
        cx,
    );
    let mut panes = compare_ui::pane_row();
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
    let path = &c.paths[index];
    compare_ui::pane(
        index,
        ComparisonPane {
            title: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            tooltip: path.display().to_string(),
            active: c.active == index,
            image: c.images[index]
                .as_ref()
                .map(|image| (image.render.clone(), image.dimensions)),
            preview: c.images[index].as_ref().is_some_and(|image| image.preview),
            camera: c.camera,
            bounds: c.areas[index],
            message: c.errors[index].clone(),
            culling: Some(ws.library.culling_of(path)),
            overlay: None,
        },
        ComparisonActions::<Workspace> {
            select: |ws, index, position, cx| {
                if let Some(c) = &mut ws.library.comparison {
                    c.active = index;
                    c.drag = position;
                }
                cx.notify();
            },
            drag: |ws, position, pressed, cx| {
                let Some(c) = &mut ws.library.comparison else {
                    return;
                };
                if !pressed {
                    c.drag = None;
                    return;
                }
                let Some(previous) = c.drag else {
                    return;
                };
                c.drag = Some(position);
                let Some(image) = &c.images[c.active] else {
                    return;
                };
                let area = c.areas[c.active].size;
                let rect = c
                    .camera
                    .image_rect(image.dimensions, [area.width.into(), area.height.into()]);
                c.camera.pan(
                    [
                        (position.x - previous.x).into(),
                        (position.y - previous.y).into(),
                    ],
                    [rect[2], rect[3]],
                );
                cx.notify();
            },
            release: |ws, _| {
                if let Some(c) = &mut ws.library.comparison {
                    c.drag = None;
                }
            },
            zoom: |ws, factor, cx| {
                if let Some(c) = &mut ws.library.comparison {
                    c.camera.zoom_by(factor);
                }
                cx.notify();
            },
            bounds: |ws, index, bounds, cx| {
                if let Some(c) = &mut ws.library.comparison {
                    if c.areas[index] != bounds {
                        c.areas[index] = bounds;
                        cx.notify();
                    }
                }
            },
        },
        cx,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_gallery::culling::{ColourLabel, CullFlag};
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
