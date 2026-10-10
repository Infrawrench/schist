//! Frame animation in the editor: the Timeline panel's operations,
//! playback, onion skins, frame thumbnails and animation export.
//!
//! Nothing here mutates the document except through the kernel's frame
//! operations, each one undoable edit. Playback and onion skins are
//! display-only: frames are composited into images (cached on each
//! frame's content key) and drawn over the canvas, so playing never
//! touches the layers, the history or the dirty flag.

use super::*;
use schist_core::animation::{self as anim, FrameResult, LoopCount};
use schist_i18n::{t, tf};

/// Longer side of a frame thumbnail, in pixels.
pub(crate) const THUMB_EDGE: u32 = 64;
/// Cap on the playback and onion-skin image size, whatever the zoom.
const DISPLAY_EDGE_MAX: u32 = 2048;
/// Rendered frame images kept, at most, before the oldest go.
const CACHE_BYTES: usize = 256 << 20;
/// Frame thumbnails re-rendered per panel paint; the rest show their
/// previous image until the next paint, so a long timeline does not stall
/// one frame of UI on many composites.
const THUMBS_PER_PAINT: usize = 4;

/// The delays the panel offers, in milliseconds. Photoshop's list.
pub(crate) const DELAYS_MS: [u32; 9] = [0, 50, 100, 200, 500, 1000, 2000, 5000, 10000];
/// The loop choices: plays, with zero meaning forever.
pub(crate) const LOOPS: [u32; 5] = [1, 2, 3, 5, 0];

/// Onion skin settings. Session-only, like the other viewer overlays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Onion {
    pub enabled: bool,
    pub before: u8,
    pub after: u8,
    pub opacity: f32,
}

impl Default for Onion {
    fn default() -> Self {
        Onion {
            enabled: false,
            before: 1,
            after: 1,
            opacity: 0.35,
        }
    }
}

pub(crate) struct Playback {
    doc: schist_core::DocumentId,
    pub frame: usize,
    plays: u32,
    generation: u64,
}

/// What a cached image was rendered as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    /// Over a checkerboard, opaque: thumbnails and playback.
    Checker,
    /// Tinted and translucent: an onion skin before or after the frame.
    Onion { after: bool, opacity: u8 },
}

struct FrameImage {
    key: u64,
    edge: u32,
    variant: Variant,
    image: Arc<RenderImage>,
    bytes: usize,
}

#[derive(Default)]
pub(crate) struct AnimationView {
    pub onion: Onion,
    pub playing: Option<Playback>,
    generation: u64,
    images: Vec<FrameImage>,
    /// The thumbnail each frame index last showed, by document.
    thumbs: Vec<Option<(u64, Arc<RenderImage>)>>,
    thumbs_doc: Option<schist_core::DocumentId>,
    /// Some thumbnails are out of date and wait for another paint.
    pub thumbs_pending: bool,
}

/// Composite straight RGBA over the panel checkerboard, as BGRA.
fn checker_bgra(rgba: &[u8], w: u32) -> Vec<u8> {
    let mut out = vec![0u8; rgba.len()];
    for (i, (p, o)) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(out.as_chunks_mut::<4>().0.iter_mut())
        .enumerate()
    {
        let (x, y) = (i as u32 % w.max(1), i as u32 / w.max(1));
        let bg = if ((x >> 3) + (y >> 3)) & 1 == 0 {
            0xFF
        } else {
            0xCC
        };
        let a = p[3] as u32;
        let mix = |c: u8| ((c as u32 * a + bg * (255 - a)) / 255) as u8;
        o.copy_from_slice(&[mix(p[2]), mix(p[1]), mix(p[0]), 255]);
    }
    out
}

fn straight_bgra(rgba: &[u8]) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], p[3]])
        .collect()
}

pub(crate) fn delay_label(ms: u32) -> String {
    let seconds = ms as f32 / 1000.0;
    let text = if ms.is_multiple_of(1000) {
        format!("{}", ms / 1000)
    } else if ms.is_multiple_of(100) {
        format!("{seconds:.1}")
    } else {
        format!("{seconds:.2}")
    };
    tf!("animation.delay_seconds", seconds = text)
}

pub(crate) fn loop_label(plays: u32) -> String {
    match plays {
        0 => t("animation.loop.forever").to_string(),
        1 => t("animation.loop.once").to_string(),
        n => format!("{n}×"),
    }
}

impl Workspace {
    /// Run one frame operation on the active document: stop playback,
    /// apply it, and say why when it did nothing.
    pub(crate) fn frame_op(
        &mut self,
        op: impl FnOnce(&mut Document) -> FrameResult,
        cx: &mut Context<Self>,
    ) {
        self.stop_playback();
        let Some(doc) = self.doc.as_mut() else { return };
        if let Err(refusal) = op(doc) {
            if let Some(why) = schist_commands_core::animation::refusal_message(refusal) {
                self.status = why.into();
            }
        }
        self.after_change(cx);
    }

    pub(crate) fn select_frame(&mut self, index: usize, cx: &mut Context<Self>) {
        self.frame_op(
            |doc| anim::select(doc, index, t("animation.history.select_frame")),
            cx,
        );
    }

    pub(crate) fn move_frame(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        self.frame_op(
            |doc| anim::reorder(doc, from, to, t("animation.history.move_frame")),
            cx,
        );
    }

    pub(crate) fn set_frame_delay(&mut self, all: bool, ms: u32, cx: &mut Context<Self>) {
        let current = self
            .doc
            .as_ref()
            .and_then(|d| d.timeline.as_ref())
            .map(|t| t.current);
        self.frame_op(
            |doc| {
                anim::set_delay(
                    doc,
                    if all { None } else { current },
                    ms,
                    t("animation.history.frame_delay"),
                )
            },
            cx,
        );
    }

    pub(crate) fn set_frame_loop(&mut self, plays: u32, cx: &mut Context<Self>) {
        let count = match plays {
            0 => LoopCount::Forever,
            n => LoopCount::Times(n),
        };
        self.frame_op(
            |doc| anim::set_loop(doc, count, t("animation.history.loop")),
            cx,
        );
    }

    pub(crate) fn toggle_new_layers_visible(&mut self, cx: &mut Context<Self>) {
        let Some(now) = self
            .doc
            .as_ref()
            .and_then(|d| d.timeline.as_ref())
            .map(|t| t.new_layers_visible)
        else {
            return;
        };
        self.frame_op(
            |doc| anim::set_new_layers_visible(doc, !now, t("animation.history.new_layers")),
            cx,
        );
    }

    /// Move the active layer within the selected frame, or with `None`,
    /// put it back where its pixels are.
    pub(crate) fn nudge_frame_offset(&mut self, by: Option<(i32, i32)>, cx: &mut Context<Self>) {
        let Some(doc) = self.doc.as_ref() else { return };
        let (Some(layer), Some(timeline)) = (doc.active_layer, doc.timeline.as_ref()) else {
            return;
        };
        let now = timeline.current_offset(layer);
        let to = by.map_or((0, 0), |(dx, dy)| (now.0 + dx, now.1 + dy));
        self.frame_op(
            |doc| anim::set_offset(doc, layer, to, t("animation.history.offset")),
            cx,
        );
    }

    pub(crate) fn toggle_onion(&mut self, cx: &mut Context<Self>) {
        self.anim.onion.enabled = !self.anim.onion.enabled;
        cx.notify();
    }

    pub(crate) fn set_onion(&mut self, onion: Onion, cx: &mut Context<Self>) {
        self.anim.onion = onion;
        cx.notify();
    }

    pub(crate) fn is_playing(&self) -> bool {
        self.anim.playing.is_some()
    }

    pub(crate) fn stop_playback(&mut self) {
        if self.anim.playing.take().is_some() {
            self.anim.generation += 1;
        }
    }

    /// Play or pause. Playback starts from the selected frame and runs on
    /// the frames' own delays, honouring the loop count; it shows rendered
    /// frames over the canvas and leaves the document alone.
    pub fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if self.is_playing() {
            self.stop_playback();
            cx.notify();
            return;
        }
        let Some(doc) = self.doc.as_ref() else { return };
        let Some(timeline) = doc.timeline.as_ref() else {
            self.status = t("animation.refuse.no_animation").into();
            cx.notify();
            return;
        };
        if timeline.frames.len() < 2 {
            return;
        }
        self.anim.generation += 1;
        let generation = self.anim.generation;
        self.anim.playing = Some(Playback {
            doc: doc.id,
            frame: timeline.current,
            plays: 0,
            generation,
        });
        cx.spawn(async move |this, cx| {
            let mut due = web_time::Instant::now();
            loop {
                let Ok(Some(delay)) = this.update(cx, |ws, _| ws.playback_delay(generation)) else {
                    break;
                };
                // Frames are due on the animation's own clock; a slow
                // render shortens the next wait instead of drifting, and a
                // stall longer than a frame restarts the clock.
                due += std::time::Duration::from_millis(delay.max(10) as u64);
                let now = web_time::Instant::now();
                if due < now {
                    due = now;
                }
                cx.background_executor().timer(due - now).await;
                let keep = this
                    .update(cx, |ws, cx| ws.advance_playback(generation, cx))
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn playback_delay(&self, generation: u64) -> Option<u32> {
        let playing = self.anim.playing.as_ref()?;
        if playing.generation != generation {
            return None;
        }
        let doc = self.doc.as_ref()?;
        Some(doc.timeline.as_ref()?.frames.get(playing.frame)?.delay_ms)
    }

    fn advance_playback(&mut self, generation: u64, cx: &mut Context<Self>) -> bool {
        let still = self.anim.playing.as_ref().is_some_and(|p| {
            p.generation == generation && self.doc.as_ref().is_some_and(|d| d.id == p.doc)
        });
        let timeline = self.doc.as_ref().and_then(|d| d.timeline.as_ref());
        let (Some(timeline), true) = (timeline, still) else {
            self.stop_playback();
            cx.notify();
            return false;
        };
        let len = timeline.frames.len();
        let plays = timeline.loop_count.plays();
        let Some(playing) = self.anim.playing.as_mut() else {
            return false;
        };
        let mut next = playing.frame + 1;
        if next >= len {
            playing.plays += 1;
            if plays.is_some_and(|n| playing.plays >= n) || len < 2 {
                self.stop_playback();
                cx.notify();
                return false;
            }
            next = 0;
        }
        playing.frame = next;
        cx.notify();
        true
    }

    /// Frame `index` rendered for display, cached on its content key.
    fn frame_image(
        &mut self,
        index: usize,
        edge: u32,
        variant: Variant,
    ) -> Option<Arc<RenderImage>> {
        let doc = self.doc.as_ref()?;
        let timeline = doc.timeline.as_ref()?.synced(&doc.tree);
        if index >= timeline.frames.len() {
            return None;
        }
        let key = timeline.frame_key(&doc.tree, index);
        if let Some(at) = self
            .anim
            .images
            .iter()
            .position(|i| i.key == key && i.edge == edge && i.variant == variant)
        {
            // Most recently used last.
            let entry = self.anim.images.remove(at);
            let image = entry.image.clone();
            self.anim.images.push(entry);
            return Some(image);
        }
        let rgba = schist_animation::render_frame(doc, index)?;
        let (mut rgba, w, h) = schist_animation::downscale(&rgba, doc.width, doc.height, edge);
        // Canvas images go through the display transform like the canvas;
        // panel thumbnails, like the layer thumbnails, do not.
        if (variant != Variant::Checker || edge > THUMB_EDGE) && self.color_managed() {
            let mut managed: Vec<f32> = rgba.iter().map(|&v| v as f32 / 255.0).collect();
            self.to_display(&mut managed);
            for (out, value) in rgba.iter_mut().zip(managed) {
                *out = schist_color::f32_to_u8(value);
            }
        }
        let bgra = match variant {
            Variant::Checker => checker_bgra(&rgba, w),
            Variant::Onion { after, opacity } => straight_bgra(&schist_animation::tint(
                &rgba,
                if after {
                    schist_animation::ONION_AFTER
                } else {
                    schist_animation::ONION_BEFORE
                },
                opacity as f32 / 255.0,
            )),
        };
        let bytes = bgra.len();
        let buffer = image::RgbaImage::from_raw(w, h, bgra)?;
        let image = Arc::new(RenderImage::new(smallvec![image::Frame::new(buffer)]));
        self.anim.images.push(FrameImage {
            key,
            edge,
            variant,
            image: image.clone(),
            bytes,
        });
        let mut total: usize = self.anim.images.iter().map(|i| i.bytes).sum();
        while total > CACHE_BYTES && self.anim.images.len() > 1 {
            let old = self.anim.images.remove(0);
            total -= old.bytes;
            self.retired_images.push(old.image);
        }
        Some(image)
    }

    /// Thumbnails for every frame of the active document's animation.
    pub(crate) fn frame_thumbnails(&mut self) -> Vec<Option<Arc<RenderImage>>> {
        let Some(doc) = self.doc.as_ref() else {
            return Vec::new();
        };
        let Some(timeline) = doc.timeline.as_ref().map(|t| t.synced(&doc.tree)) else {
            return Vec::new();
        };
        if self.anim.thumbs_doc != Some(doc.id) {
            self.anim.thumbs.clear();
            self.anim.thumbs_doc = Some(doc.id);
        }
        let keys: Vec<u64> = (0..timeline.frames.len())
            .map(|i| timeline.frame_key(&doc.tree, i))
            .collect();
        self.anim.thumbs.resize(keys.len(), None);
        let mut budget = if self.pointer_down {
            0
        } else {
            THUMBS_PER_PAINT
        };
        let mut stale = false;
        for (i, key) in keys.iter().enumerate() {
            if self.anim.thumbs[i].as_ref().is_some_and(|(k, _)| k == key) {
                continue;
            }
            if budget == 0 {
                stale = true;
                continue;
            }
            budget -= 1;
            if let Some(image) = self.frame_image(i, THUMB_EDGE, Variant::Checker) {
                self.anim.thumbs[i] = Some((*key, image));
            }
        }
        if stale {
            self.anim.thumbs_pending = true;
        }
        self.anim
            .thumbs
            .iter()
            .map(|t| t.as_ref().map(|(_, image)| image.clone()))
            .collect()
    }

    /// The images to draw over the canvas: the playing frame, or the
    /// onion skins around the selected one. Drawn unrotated, so a rotated
    /// view shows neither.
    pub(super) fn animation_overlays(&mut self, display_edge: u32) -> Vec<Arc<RenderImage>> {
        if self.rotation != 0.0 {
            return Vec::new();
        }
        let Some(doc) = self.doc.as_ref() else {
            return Vec::new();
        };
        let Some(timeline) = doc.timeline.as_ref() else {
            return Vec::new();
        };
        let edge = display_edge
            .min(doc.width.max(doc.height))
            .clamp(1, DISPLAY_EDGE_MAX);
        if let Some(frame) = self
            .anim
            .playing
            .as_ref()
            .filter(|p| p.doc == doc.id)
            .map(|p| p.frame)
        {
            return self
                .frame_image(frame, edge, Variant::Checker)
                .into_iter()
                .collect();
        }
        let onion = self.anim.onion;
        if !onion.enabled {
            return Vec::new();
        }
        let (current, len) = (timeline.current, timeline.frames.len());
        let opacity = (onion.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        let mut out = Vec::new();
        // Farthest first, so the nearest frames sit on top.
        for k in (1..=onion.before as usize).rev() {
            if let Some(index) = current.checked_sub(k) {
                let variant = Variant::Onion {
                    after: false,
                    opacity: (opacity as usize / k) as u8,
                };
                out.extend(self.frame_image(index, edge, variant));
            }
        }
        for k in (1..=onion.after as usize).rev() {
            if current + k < len {
                let variant = Variant::Onion {
                    after: true,
                    opacity: (opacity as usize / k) as u8,
                };
                out.extend(self.frame_image(current + k, edge, variant));
            }
        }
        out
    }

    /// Open File ▸ Export Animation, or say why there is nothing to export.
    pub fn open_export_animation(&mut self, cx: &mut Context<Self>) {
        if self
            .doc
            .as_ref()
            .and_then(|d| d.timeline.as_ref())
            .is_none()
        {
            self.status = t("animation.refuse.no_animation").into();
            cx.notify();
            return;
        }
        self.open_modal(
            Modal::ExportAnimation {
                options: schist_animation::ExportOptions::default(),
            },
            cx,
        );
    }

    /// Render every frame, encode, and write or download the file.
    pub fn export_animation(
        &mut self,
        options: schist_animation::ExportOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_playback();
        let Some(doc) = self.doc.as_ref() else { return };
        let stem = std::path::Path::new(&doc.title)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into());
        let suggested = format!("{stem}.{}", options.format.extension());
        let encode = move |doc: &Document| -> anyhow::Result<Vec<u8>> {
            let timeline = doc
                .timeline
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("{}", t("animation.refuse.no_animation")))?;
            let frames = schist_animation::render_frames(doc)
                .ok_or_else(|| anyhow::anyhow!("{}", t("animation.refuse.no_animation")))?;
            schist_animation::encode(
                &frames,
                doc.width,
                doc.height,
                timeline.loop_count,
                &options,
            )
        };
        #[cfg(target_arch = "wasm32")]
        {
            let _ = window;
            let Some(name) = crate::web::prompt_string(t("workspace.export.prompt"), &suggested)
            else {
                return;
            };
            let result = encode(doc).and_then(|bytes| crate::web::download_bytes(&name, &bytes));
            self.status = match result {
                Ok(()) => tf!("workspace.export.done", name = name).into(),
                Err(err) => tf!("workspace.export.failed", error = err).into(),
            };
            cx.notify();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = doc
                .path
                .as_ref()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("."));
            let rx = self.prompt_for_new_path(&dir, Some(&suggested), cx);
            cx.spawn_in(window, async move |this, cx| {
                if let Ok(Ok(Some(path))) = rx.await {
                    this.update_in(cx, |ws, _window, cx| {
                        let result = ws
                            .doc
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("{}", t("common.no_document")))
                            .and_then(encode)
                            .and_then(|bytes| Ok(std::fs::write(&path, bytes)?));
                        ws.status = match result {
                            Ok(()) => {
                                tf!("workspace.export.done", name = crate::ui::shown_path(&path))
                                    .into()
                            }
                            Err(err) => tf!("workspace.export.failed", error = err).into(),
                        };
                        cx.notify();
                    })
                    .ok();
                }
            })
            .detach();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_checker() {
        assert!(delay_label(100).contains("0.1"));
        assert!(delay_label(2000).contains('2'));
        assert_eq!(loop_label(3), "3×");
        let bgra = checker_bgra(&[255, 0, 0, 255, 0, 0, 0, 0], 2);
        assert_eq!(&bgra[..4], &[0, 0, 255, 255]);
        assert_eq!(&bgra[4..], &[255, 255, 255, 255]);
    }
}
