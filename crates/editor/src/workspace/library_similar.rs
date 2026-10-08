//! Similar originals and capture bursts, with reversible review decisions.
use super::*;
use gpui::{img, prelude::FluentBuilder as _, StatefulInteractiveElement as _, StyledImage as _};
use schist_gallery::similar::{self, Cache, Choice, Decisions, Group, Mode, Photo};
use schist_i18n::{t, tf};
use schist_ui::Button;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize};

/// What the review panel is showing: the current view's similar photos and
/// bursts, or Find Duplicates over the whole library.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ReviewKind {
    Similar,
    Duplicates,
}

pub(super) struct SimilarReview {
    pub open: bool,
    pub(super) kind: ReviewKind,
    mode: Mode,
    pub(super) threshold: u32,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) progress: Arc<AtomicUsize>,
    pub(super) running: bool,
    /// Sources the scan will inspect; a duplicate scan learns it after
    /// listing the files, so it is shared with the worker.
    pub(super) total: Arc<AtomicUsize>,
    pub(super) failed: usize,
    pub(super) cancelled: bool,
    pub(super) photos: Vec<Photo>,
    pub(super) groups: Vec<Group>,
    pub(super) group: usize,
    pub(super) candidate: usize,
    images: [Option<Arc<RenderImage>>; 2],
    loading: bool,
    preview_cancel: Arc<AtomicBool>,
    revision: u64,
    pub(super) decisions: Decisions,
    pub(super) decisions_loaded: bool,
    pub(super) error: Option<String>,
    /// Duplicates: whether each group is byte-identical, and the SHA-256 of
    /// exact members (by photo index) for re-verification before trashing.
    pub(super) exact: Vec<bool>,
    pub(super) sha: std::collections::HashMap<usize, schist_gallery::duplicates::Sha256Digest>,
    /// More stills than near-duplicate matching covers.
    pub(super) near_limited: bool,
    /// The group whose "move to trash" was clicked once and awaits a second.
    pub(super) trash_armed: Option<usize>,
    /// A comparison opened from the review returns to it on close.
    pub(super) resume: bool,
}
impl Default for SimilarReview {
    fn default() -> Self {
        Self {
            open: false,
            kind: ReviewKind::Similar,
            mode: Mode::Visual,
            threshold: 6,
            cancel: Arc::default(),
            progress: Arc::default(),
            running: false,
            total: Arc::default(),
            failed: 0,
            cancelled: false,
            photos: vec![],
            groups: vec![],
            group: 0,
            candidate: 1,
            images: [None, None],
            loading: false,
            preview_cancel: Arc::default(),
            revision: 0,
            decisions: Decisions::default(),
            decisions_loaded: false,
            error: None,
            exact: vec![],
            sha: Default::default(),
            near_limited: false,
            trash_armed: None,
            resume: false,
        }
    }
}
impl SimilarReview {
    /// Reset for a fresh scan of `total` sources.
    pub(super) fn begin_scan(&mut self, total: usize) {
        self.preview_cancel.store(true, Ordering::Relaxed);
        self.total = Arc::new(AtomicUsize::new(total));
        self.cancel = Arc::default();
        self.progress = Arc::default();
        self.running = true;
        self.cancelled = false;
        self.error = None;
        self.groups.clear();
        self.photos.clear();
        self.exact.clear();
        self.sha.clear();
        self.near_limited = false;
        self.trash_armed = None;
        self.images = [None, None];
        self.revision += 1;
    }
    pub(super) fn finish_decisions(&mut self, decisions: std::io::Result<Decisions>) {
        match decisions {
            Ok(decisions) => {
                self.decisions = decisions;
                self.decisions_loaded = true;
            }
            Err(error) => {
                self.decisions_loaded = false;
                self.error = Some(review_io_error(error));
            }
        }
    }
}
/// The small, flattened decode similar-photo signatures are computed from.
pub(super) fn signature_image(path: &Path) -> Option<image::RgbImage> {
    let preview = schist_preview::render_file(path, 256).ok()?;
    image::RgbaImage::from_raw(preview.width, preview.height, preview.rgba).map(|rgba| {
        image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
            let p = rgba.get_pixel(x, y);
            let a = p[3] as u32;
            image::Rgb([0, 1, 2].map(|c| ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8))
        })
    })
}
/// Capture seconds from the cached EXIF/XMP metadata, never the file clock.
pub(super) fn capture_seconds(path: &Path, stamp: &similar::Stamp) -> Option<i64> {
    let cache = schist_gallery::thumb_cache_path(path, stamp.seconds)
        .map(|p| p.with_extension(format!("{}-{}.png", stamp.nanos, stamp.bytes)));
    schist_gallery::photo_meta(&cache, path)
        .taken
        .as_deref()
        .and_then(similar::capture_seconds)
}
pub(super) fn review_io_error(error: std::io::Error) -> String {
    log::warn!("similar review persistence failed: {error}");
    let detail = match error.kind() {
        std::io::ErrorKind::PermissionDenied => t("common.permission_denied"),
        std::io::ErrorKind::NotFound => t("common.file_not_found"),
        std::io::ErrorKind::InvalidData => t("common.unsupported_format"),
        _ => t("common.not_available"),
    };
    tf!("library.ops.save_failed", error = detail)
}
pub(super) fn decision_path() -> Option<PathBuf> {
    Some(schist_gallery::library_path()?.with_file_name("similar-review.json"))
}

impl Workspace {
    /// The sidebar's Similar photos: the current view, not the library.
    pub(super) fn open_similar_review(&mut self, cx: &mut Context<Self>) {
        if self.library.similar.kind != ReviewKind::Similar && !self.library.similar.running {
            self.library.similar.kind = ReviewKind::Similar;
            self.library.similar.photos.clear();
            self.library.similar.groups.clear();
        }
        self.show_review(cx);
    }
    pub(super) fn show_review(&mut self, cx: &mut Context<Self>) {
        self.library.comparison = None;
        self.library.map_view = false;
        self.library.viewer = None;
        self.library.similar.open = true;
        self.library.selected.clear();
        if self.library.similar.photos.is_empty() && !self.library.similar.running {
            self.rescan_review(cx);
        } else if !self.library.similar.running {
            self.load_similar_pair(cx);
        }
        cx.notify();
    }
    pub(super) fn close_similar_review(&mut self) {
        self.library.similar.open = false;
        self.library
            .similar
            .preview_cancel
            .store(true, Ordering::Relaxed);
        self.library.similar.cancel.store(true, Ordering::Relaxed);
        self.library.similar.revision += 1;
        self.library.similar.images = [None, None];
        self.library.similar.loading = false;
    }
    pub(super) fn similar_review_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.library.similar.open || self.library.viewer.is_some() || self.library.search.active
        {
            return false;
        }
        match ev.keystroke.key.as_str() {
            "left" | "right" => {
                let state = &mut self.library.similar;
                if let Some(group) = state.groups.get(state.group) {
                    let next = if ev.keystroke.key == "left" {
                        state.candidate.saturating_sub(1).max(1)
                    } else {
                        (state.candidate + 1).min(group.photos.len() - 1)
                    };
                    if next != state.candidate {
                        state.candidate = next;
                        self.load_similar_pair(cx);
                        cx.notify();
                    }
                }
            }
            "space" | "enter" | "delete" | "backspace" | "up" | "down" => {}
            _ => return false,
        }
        true
    }
    /// Repeat whichever scan the panel is showing.
    pub(super) fn rescan_review(&mut self, cx: &mut Context<Self>) {
        match self.library.similar.kind {
            ReviewKind::Similar => self.scan_similar(cx),
            ReviewKind::Duplicates => self.scan_duplicates(cx),
        }
    }
    /// Repaint while a scan runs. Progress is actual completed source
    /// inspections, including cache hits and failures; the timer only
    /// repaints, it does not invent progress.
    pub(super) fn similar_progress_ticker(&mut self, cx: &mut Context<Self>) {
        let ticker_cancel = self.library.similar.cancel.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            let active = this
                .update(cx, |ws, cx| {
                    cx.notify();
                    ws.library.similar.running
                        && Arc::ptr_eq(&ws.library.similar.cancel, &ticker_cancel)
                })
                .unwrap_or(false);
            if !active {
                break;
            }
        })
        .detach();
    }
    fn scan_similar(&mut self, cx: &mut Context<Self>) {
        if self.library.similar.running {
            return;
        }
        let mut paths = self.gallery_flat_order();
        paths.retain(|p| !schist_gallery::is_video(p));
        paths.sort();
        paths.dedup();
        let state = &mut self.library.similar;
        state.begin_scan(paths.len());
        let (cancel, progress, mode, threshold) = (
            state.cancel.clone(),
            state.progress.clone(),
            state.mode,
            state.threshold,
        );
        self.similar_progress_ticker(cx);
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let cache_path =
                        schist_gallery::state_dir().map(|p| p.join("schist/similar-v1.json"));
                    let mut cache = cache_path.as_deref().map(Cache::load).unwrap_or_default();
                    let photos =
                        cache.scan(&paths, &cancel, &progress, signature_image, capture_seconds);
                    if let Some(path) = cache_path {
                        if let Err(error) = cache.save(&path) {
                            log::warn!("similar cache write failed: {error}");
                        }
                    }
                    let groups = similar::groups(&photos, mode, threshold, &cancel);
                    let decisions = decision_path()
                        .ok_or_else(|| std::io::Error::other(t("common.not_available")))
                        .and_then(|p| Decisions::load(&p));
                    (photos, groups, cancel.load(Ordering::Relaxed), decisions)
                })
                .await;
            this.update(cx, |ws, cx| {
                let state = &mut ws.library.similar;
                state.running = false;
                state.cancelled = outcome.2;
                state.failed = state
                    .progress
                    .load(Ordering::Relaxed)
                    .saturating_sub(outcome.0.iter().filter(|p| p.signature.is_some()).count());
                if !outcome.2 {
                    state.photos = outcome.0;
                    state.groups = outcome.1;
                }
                state.group = 0;
                state.candidate = 1;
                state.finish_decisions(outcome.3);
                ws.load_similar_pair(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
    pub(super) fn load_similar_pair(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.library.similar;
        state.revision += 1;
        state.preview_cancel.store(true, Ordering::Relaxed);
        state.preview_cancel = Arc::default();
        let cancel = state.preview_cancel.clone();
        state.images = [None, None];
        let Some(group) = state.groups.get(state.group) else {
            state.loading = false;
            return;
        };
        let indices = [group.photos[0], group.photos[state.candidate]];
        let paths = indices.map(|i| state.photos[i].path.clone());
        let revision = state.revision;
        state.loading = true;
        cx.spawn(async move |this, cx| {
            let previews = cx
                .background_executor()
                .spawn(async move {
                    paths.map(|path| {
                        if cancel.load(Ordering::Relaxed) {
                            None
                        } else {
                            schist_preview::render_file(&path, 1600).ok()
                        }
                    })
                })
                .await;
            this.update(cx, |ws, cx| {
                let state = &mut ws.library.similar;
                if state.revision != revision {
                    return;
                }
                state.images = previews.map(|p| {
                    p.and_then(|p| super::library::rgba_to_render_image(p.width, p.height, p.rgba))
                });
                state.loading = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
    fn similar_choice(
        &mut self,
        index: usize,
        revision: u64,
        choice: Option<Choice>,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.library.similar;
        // A click queued before a refresh or group change belongs to that
        // rendered pair, not whichever source now occupies the same index.
        if state.revision != revision || state.running || !state.decisions_loaded {
            return;
        }
        let Some(group) = state.groups.get(state.group) else {
            return;
        };
        let mut next = state.decisions.clone();
        if !next.set(&state.photos, group, index, choice) {
            state.error = Some(t("library.similar.stale").into());
        } else {
            match decision_path()
                .ok_or_else(|| std::io::Error::other(t("common.not_available")))
                .and_then(|p| next.save(&p))
            {
                Ok(()) => {
                    state.decisions = next;
                    state.error = None;
                }
                Err(error) => state.error = Some(review_io_error(error)),
            }
        }
        cx.notify();
    }
}

/// How tall each photo of the pair under review is shown.
const PREVIEW_HEIGHT: f32 = 360.0;

fn control_group() -> gpui::Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .p_0p5()
        .rounded_sm()
        .bg(gpui::rgb(super::gallery_chrome::pal().tray_bg))
}

pub(super) fn render(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let p = super::gallery_chrome::pal();
    // Clipping and focus-peaking layers for the two previews, which are
    // PREVIEW_HEIGHT tall and as wide as their aspect allows.
    let overlays = ws.library.similar.images.clone().map(|image| {
        let image = image?;
        let size = image.size(0);
        let aspect = size.width.0 as f32 / size.height.0.max(1) as f32;
        ws.gallery_overlay(&image, PREVIEW_HEIGHT * aspect.max(1.0))
    });
    let state = &ws.library.similar;
    let running = state.running;
    let dupes = state.kind == ReviewKind::Duplicates;
    let mut modes = control_group();
    for (mode, key) in [
        (Mode::Visual, "library.similar.visual"),
        (Mode::Burst, "library.similar.burst"),
    ] {
        modes = modes.child(
            Button::new(key, t(key))
                .ghost()
                .px_2()
                .active(state.mode == mode)
                .disabled(running)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    if ws.library.similar.mode != mode {
                        ws.library.similar.mode = mode;
                        ws.scan_similar(cx);
                    }
                })),
        );
    }
    let mut controls = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .bg(gpui::rgb(p.chrome_bg))
        .border_b_1()
        .border_color(gpui::rgb(p.chrome_edge))
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .mr_2()
                .child(t(if dupes {
                    "library.duplicates.title"
                } else {
                    "library.similar.title"
                })),
        )
        .when(!dupes, |el| el.child(modes));
    if dupes || state.mode == Mode::Visual {
        let mut thresholds = control_group().child(
            div()
                .px_2()
                .text_color(gpui::rgb(p.text_dim))
                .child(t("common.threshold")),
        );
        for threshold in [2, 6, 10] {
            thresholds = thresholds.child(
                Button::new(
                    ("similar-threshold", threshold as usize),
                    format!("{threshold}/64"),
                )
                .ghost()
                .px_2()
                .active(state.threshold == threshold)
                .disabled(running)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    if ws.library.similar.threshold != threshold {
                        ws.library.similar.threshold = threshold;
                        ws.rescan_review(cx);
                    }
                })),
            );
        }
        controls = controls.child(thresholds);
    }
    controls = controls.child(div().flex_grow()).child(
        Button::new("similar-scan", t("common.refresh"))
            .ghost()
            .px_2()
            .disabled(running)
            .on_click(cx.listener(|ws, _, _, cx| ws.rescan_review(cx))),
    );
    let mut status = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .text_size(px(11.0))
        .text_color(gpui::rgb(p.text_dim))
        .child(t(if dupes {
            "library.duplicates.help"
        } else {
            "library.similar.help"
        }))
        .child(div().flex_grow());
    if running || state.cancelled {
        status = status
            .child(t(if running {
                "common.in_progress"
            } else {
                "common.cancelled"
            }))
            .child(format!(
                "{}/{}",
                state.progress.load(Ordering::Relaxed),
                state.total.load(Ordering::Relaxed)
            ));
    }
    if running {
        status = status.child(
            Button::new("similar-cancel", t("common.cancel"))
                .ghost()
                .px_2()
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.library.similar.cancel.store(true, Ordering::Relaxed);
                    cx.notify();
                })),
        );
    } else {
        status = status.child(format!("{} {}", t("common.group"), state.groups.len()));
        if state.failed > 0 {
            status = status.child(format!("{} {}", t("common.failed"), state.failed));
        }
        if dupes && state.near_limited {
            status = status.child(t("library.duplicates.near_limit"));
        }
        if !dupes && state.mode == Mode::Burst {
            let undated = state.photos.iter().filter(|p| p.captured.is_none()).count();
            if undated > 0 {
                status = status.child(format!("{} {undated}", t("library.month.undated")));
            }
        }
    }
    let mut body = div()
        .id("similar-review-body")
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .overflow_y_scroll()
        .gap_2()
        .p_3()
        .child(status)
        .children(state.error.clone().map(|error| {
            div()
                .p_2()
                .rounded_sm()
                .bg(gpui::rgb(p.chrome_bg))
                .child(error)
        }));
    let mut content = div()
        .id("similar-review")
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .text_color(gpui::rgb(p.text))
        .child(controls);
    let Some(group) = state.groups.get(state.group) else {
        if !running && !state.cancelled {
            body = body.child(
                div()
                    .p_3()
                    .text_color(gpui::rgb(p.text_dim))
                    .child(t("common.ready")),
            );
        }
        return content.child(body).into_any_element();
    };
    let group_len = group.photos.len();
    let group_count = state.groups.len();
    let group_index = state.group;
    let candidate = state.candidate;
    let left = &state.photos[group.photos[0]];
    let right = &state.photos[group.photos[candidate]];
    let exact = dupes && state.exact.get(state.group).copied().unwrap_or(false);
    let evidence = match state.mode {
        _ if exact => t("library.duplicates.identical").to_string(),
        _ if dupes => format!(
            "{}/64",
            left.signature
                .as_ref()
                .zip(right.signature.as_ref())
                .map(|(a, b)| a.distance(b))
                .unwrap_or(64)
        ),
        Mode::Visual => format!(
            "{}/64",
            left.signature
                .as_ref()
                .zip(right.signature.as_ref())
                .map(|(a, b)| a.distance(b))
                .unwrap_or(64)
        ),
        Mode::Burst => format!(
            "{} {}",
            left.captured
                .zip(right.captured)
                .map(|(a, b)| b - a)
                .unwrap_or(0),
            t("common.unit.seconds_suffix")
        ),
    };
    body = body.child(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(
                control_group()
                    .child(div().px_2().child(format!(
                        "{} {}/{}",
                        t("common.group"),
                        group_index + 1,
                        group_count
                    )))
                    .child(
                        Button::new("similar-prev-group", "‹")
                            .ghost()
                            .px_2()
                            .tooltip(t("common.back"), None)
                            .disabled(group_index == 0)
                            .on_click(cx.listener(|ws, _, _, cx| {
                                ws.library.similar.group -= 1;
                                ws.library.similar.candidate = 1;
                                ws.load_similar_pair(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("similar-next-group", "›")
                            .ghost()
                            .px_2()
                            .tooltip(t("common.next"), None)
                            .disabled(group_index + 1 == group_count)
                            .on_click(cx.listener(|ws, _, _, cx| {
                                ws.library.similar.group += 1;
                                ws.library.similar.candidate = 1;
                                ws.load_similar_pair(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                control_group()
                    .child(div().px_2().child(format!(
                        "{} {}/{}",
                        t("common.photo"),
                        candidate + 1,
                        group_len
                    )))
                    .child(
                        Button::new("similar-prev-photo", "‹")
                            .ghost()
                            .px_2()
                            .tooltip(t("common.back"), Some("←".into()))
                            .disabled(candidate == 1)
                            .on_click(cx.listener(|ws, _, _, cx| {
                                ws.library.similar.candidate -= 1;
                                ws.load_similar_pair(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("similar-next-photo", "›")
                            .ghost()
                            .px_2()
                            .tooltip(t("common.next"), Some("→".into()))
                            .disabled(candidate + 1 == group_len)
                            .on_click(cx.listener(|ws, _, _, cx| {
                                ws.library.similar.candidate += 1;
                                ws.load_similar_pair(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex_grow())
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(gpui::rgb(p.text_dim))
                    .child(format!("{} {evidence}", t("common.distance"))),
            ),
    );
    if dupes {
        body = body.child(super::library_duplicates::group_actions(state, cx));
    }
    let revision = state.revision;
    let mut pair = div().flex().flex_row().flex_wrap().gap_2();
    for (pane, index) in [group.photos[0], group.photos[candidate]]
        .into_iter()
        .enumerate()
    {
        let photo = &state.photos[index];
        let choice = state.decisions.get(photo);
        let keep = choice == Some(Choice::Keep);
        let reject = choice == Some(Choice::Reject);
        let can_reject = group.photos.iter().any(|other| {
            *other != index && state.decisions.get(&state.photos[*other]) != Some(Choice::Reject)
        });
        let filename = photo
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut card = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(220.0))
            .rounded_sm()
            .overflow_hidden()
            .border_1()
            .border_color(gpui::rgb(p.chrome_edge))
            .bg(gpui::rgb(p.chrome_bg))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .id(("similar-filename", pane))
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .tooltip(crate::ui::tip(photo.path.display().to_string(), None))
                            .child(filename),
                    )
                    .children(
                        if dupes {
                            (pane == 0).then_some("library.duplicates.suggested")
                        } else {
                            choice.map(|choice| match choice {
                                Choice::Keep => "library.similar.keep",
                                Choice::Reject => "library.similar.reject",
                            })
                        }
                        .map(|key| {
                            div()
                                .flex_none()
                                .text_size(px(11.0))
                                .text_color(gpui::rgb(p.text_dim))
                                .child(t(key))
                        }),
                    ),
            );
        let mut preview = div()
            .flex()
            .items_center()
            .justify_center()
            .relative()
            .h(px(PREVIEW_HEIGHT))
            .overflow_hidden()
            .bg(gpui::rgb(p.grid_bg));
        if let Some(image) = &state.images[pane] {
            preview = preview.child(
                img(image.clone())
                    .size_full()
                    .object_fit(gpui::ObjectFit::Contain),
            );
            // Same box and fit, so it lands exactly over the photo.
            if let Some(layer) = &overlays[pane] {
                preview = preview.child(
                    img(layer.clone())
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .object_fit(gpui::ObjectFit::Contain),
                );
            }
        } else {
            preview = preview.child(div().text_color(gpui::rgb(p.text_dim)).child(t(
                if state.loading {
                    "common.loading"
                } else {
                    "library.cell.no_preview"
                },
            )));
        }
        let path = photo.path.clone();
        let mut buttons = div().flex().flex_wrap().items_center().gap_1().p_1();
        if dupes {
            // The duplicate finder decides per group; a pane only picks
            // which member is kept.
            buttons = buttons.when(pane == 1, |el| {
                el.child(
                    Button::new("dupes-keeper", t("library.duplicates.make_keeper"))
                        .ghost()
                        .px_2()
                        .disabled(state.running)
                        .on_click(
                            cx.listener(move |ws, _, _, cx| ws.duplicates_set_keeper(index, cx)),
                        ),
                )
            });
        } else {
            buttons = buttons
                .child(
                    Button::new(("similar-keep", pane), t("library.similar.keep"))
                        .ghost()
                        .px_2()
                        .active(keep)
                        .disabled(!state.decisions_loaded)
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            if !keep {
                                ws.similar_choice(index, revision, Some(Choice::Keep), cx);
                            }
                        })),
                )
                .child(
                    Button::new(("similar-reject", pane), t("library.similar.reject"))
                        .ghost()
                        .px_2()
                        .active(reject)
                        .disabled((!reject && !can_reject) || !state.decisions_loaded)
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            if !reject {
                                ws.similar_choice(index, revision, Some(Choice::Reject), cx);
                            }
                        })),
                )
                .child(
                    Button::new(("similar-clear", pane), t("common.reset"))
                        .ghost()
                        .px_2()
                        .disabled(choice.is_none() || !state.decisions_loaded)
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.similar_choice(index, revision, None, cx)
                        })),
                );
        }
        card = card.child(preview).child(
            buttons.child(div().flex_grow()).child(
                Button::new(("similar-open", pane), t("common.edit"))
                    .ghost()
                    .px_2()
                    .on_click(
                        cx.listener(move |ws, _, _, cx| ws.open_from_gallery(path.clone(), cx)),
                    ),
            ),
        );
        pair = pair.child(card);
    }
    content = content.child(body.child(pair));
    content.into_any_element()
}
