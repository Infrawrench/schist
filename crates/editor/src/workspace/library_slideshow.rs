//! The gallery slideshow: whatever the grid is showing — the selection,
//! a folder, a bucket or smart album, a person, a search — played
//! fullscreen, one photo at a time.
//!
//! The order, the preload window and the Ken Burns framing are worked
//! out in `schist_gallery::slideshow`; this is the window's half: the
//! decodes (at the screen's size, never the original's), the clock,
//! the keys, and the drawing. The culling keys write through the same
//! path as the grid's, so a show doubles as a culling pass: 0–5 rate,
//! P/X/U flag, 6–9/M/L label the photo on screen.

use super::gallery_chrome::{gallery_button, pal};
use super::*;
use gpui::img;
use schist_gallery::people::FaceRect;
use schist_gallery::slideshow::{
    self as show, Playlist, SlideshowSettings, Transition, VideoSlides, KEEP_BEHIND, PRELOAD_AHEAD,
};
use schist_i18n::{t, tf, tn};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How often the clock looks at the show: smooth enough for a pan and
/// a fade, and the only wake-ups a running show costs.
const FRAME: Duration = Duration::from_millis(33);
/// How long the controls stay up after the pointer last moved.
const CONTROLS_LINGER: Duration = Duration::from_millis(2500);
/// How long a decision made from the keyboard is shown when the
/// caption, which would show it anyway, is off.
const FLASH_LINGER: Duration = Duration::from_millis(1500);

/// A decoded slide at display size.
pub(super) struct SlideImage {
    render: Arc<RenderImage>,
    size: (f32, f32),
    /// Where Ken Burns moves to: the faces' middle, or the centre.
    focus: (f32, f32),
    /// The photo's name (a virtual copy's own) and its XMP caption,
    /// read alongside the decode rather than per frame.
    title: String,
    caption: Option<String>,
}

enum Slot {
    Loading,
    Ready(SlideImage),
    Failed,
}

/// The slide that is fading out, and where its pan had got to.
struct Leaving {
    item: usize,
    t: f32,
    zoom_in: bool,
}

/// A video slide playing silently.
struct SlideVideo {
    item: usize,
    job: Arc<crate::video::Job>,
    frame: Option<(Arc<RenderImage>, (f32, f32))>,
    /// Where playback had got to, so a pause can resume there.
    time: f64,
    done: bool,
}

pub struct Slideshow {
    items: Vec<PathBuf>,
    playlist: Playlist,
    settings: SlideshowSettings,
    slides: FxHashMap<usize, Slot>,
    /// When the slide on show came up; moved forward over pauses, so
    /// `now - shown_at` is always the time it has been watched.
    shown_at: Instant,
    paused_at: Option<Instant>,
    leaving: Option<Leaving>,
    /// The longest edge slides are decoded at; 0 until the first frame
    /// has measured the screen.
    edge: u32,
    /// Cleared on exit: decodes and the clock still in flight see it
    /// and drop what they were doing.
    alive: Arc<AtomicBool>,
    ticking: bool,
    /// The captions, toggled with I.
    captions: bool,
    ended: bool,
    controls_until: Option<Instant>,
    flash: Option<(String, Instant)>,
    video: Option<SlideVideo>,
    /// The show asked for fullscreen; the next frame does it (that is
    /// where the window is to hand).
    want_fullscreen: bool,
    entered_fullscreen: bool,
}

impl Slideshow {
    fn elapsed(&self) -> f32 {
        let now = self.paused_at.unwrap_or_else(Instant::now);
        now.saturating_duration_since(self.shown_at).as_secs_f32()
    }

    fn current(&self) -> Option<usize> {
        self.playlist.current()
    }

    fn zoom_in(&self) -> bool {
        self.playlist.position().is_multiple_of(2)
    }

    fn current_ready(&self) -> bool {
        match self.current() {
            Some(item) if schist_gallery::is_video(&self.items[item]) => self
                .video
                .as_ref()
                .is_some_and(|v| v.item == item && (v.frame.is_some() || v.done)),
            Some(item) => matches!(self.slides.get(&item), Some(Slot::Ready(_) | Slot::Failed)),
            None => false,
        }
    }

    /// Whether anything on screen is moving, so the clock should repaint.
    fn animating(&self) -> bool {
        if self.paused_at.is_some() {
            return false;
        }
        let fading = self.leaving.is_some() && self.elapsed() < self.settings.fade_seconds();
        fading || self.settings.transition == Transition::KenBurns
    }
}

impl Workspace {
    /// Whether a slideshow has the screen.
    pub(crate) fn slideshow_active(&self) -> bool {
        self.library.slideshow.is_some()
    }

    /// The photo the slideshow is showing — what the culling keys act on.
    pub(super) fn slideshow_photo(&self) -> Option<PathBuf> {
        let show = self.library.slideshow.as_ref()?;
        show.current().map(|i| show.items[i].clone())
    }

    /// The slides the gallery would play right now, and where to start:
    /// a selection of several plays just those; otherwise everything on
    /// show — the folder, bucket, smart album, person or search — from
    /// the selected photo onwards.
    pub(super) fn slideshow_sources(&self) -> (Vec<PathBuf>, usize) {
        let order = self.gallery_flat_order();
        let selected: Vec<PathBuf> = order
            .iter()
            .filter(|p| self.library.is_selected(p))
            .cloned()
            .collect();
        if selected.len() > 1 {
            return (selected, 0);
        }
        let start = self
            .library
            .lead_selected()
            .and_then(|lead| order.iter().position(|p| p == lead))
            .unwrap_or(0);
        (order, start)
    }

    /// Ask how the show should run (the options are remembered), then
    /// start it.
    pub(super) fn open_slideshow_dialog(
        &mut self,
        photos: Vec<PathBuf>,
        start: usize,
        cx: &mut Context<Self>,
    ) {
        self.library.context = None;
        if photos.is_empty() {
            self.status = t("slideshow.nothing").into();
            cx.notify();
            return;
        }
        let settings = self.library.slideshow_settings.clone();
        self.open_modal(
            Modal::Slideshow {
                photos,
                start,
                settings,
            },
            cx,
        );
    }

    /// The slideshow from the gallery's current view.
    pub(crate) fn gallery_slideshow(&mut self, cx: &mut Context<Self>) {
        let (photos, start) = self.slideshow_sources();
        self.open_slideshow_dialog(photos, start, cx);
    }

    /// Begin a show. Videos are left out unless the options ask for
    /// them; the starting photo stays the starting photo either way.
    pub(super) fn start_slideshow(
        &mut self,
        photos: Vec<PathBuf>,
        start: usize,
        settings: SlideshowSettings,
        cx: &mut Context<Self>,
    ) {
        let settings = settings.sanitized();
        let start_path = photos.get(start).cloned();
        let items: Vec<PathBuf> = match settings.videos {
            VideoSlides::Skip => photos
                .into_iter()
                .filter(|p| !schist_gallery::is_video(p))
                .collect(),
            VideoSlides::Play => photos,
        };
        if items.is_empty() {
            self.status = t("slideshow.nothing").into();
            cx.notify();
            return;
        }
        let start = start_path
            .and_then(|s| items.iter().position(|p| *p == s))
            .unwrap_or(0);
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        let playlist = Playlist::new(items.len(), start, settings.shuffle, settings.repeat, seed);
        self.stop_slideshow_quietly();
        self.library.context = None;
        self.library.search.active = false;
        self.library.slideshow = Some(Slideshow {
            items,
            playlist,
            captions: settings.captions,
            settings,
            slides: FxHashMap::default(),
            shown_at: Instant::now(),
            paused_at: None,
            leaving: None,
            edge: 0,
            alive: Arc::new(AtomicBool::new(true)),
            ticking: false,
            ended: false,
            controls_until: Some(Instant::now() + CONTROLS_LINGER),
            flash: None,
            video: None,
            want_fullscreen: true,
            entered_fullscreen: false,
        });
        self.ensure_slideshow_ticker(cx);
        cx.notify();
    }

    /// Tear the show down without touching the window.
    fn stop_slideshow_quietly(&mut self) -> Option<Slideshow> {
        let mut show = self.library.slideshow.take()?;
        show.alive.store(false, Ordering::Release);
        if let Some(video) = show.video.take() {
            video.job.cancel();
            if let Some((frame, _)) = video.frame {
                self.retired_images.push(frame);
            }
        }
        for (_, slot) in show.slides.drain() {
            if let Slot::Ready(slide) = slot {
                self.retired_images.push(slide.render);
            }
        }
        Some(show)
    }

    /// End the show because the gallery is going away: the next frame
    /// takes the window out of fullscreen if the show put it there.
    pub(super) fn drop_slideshow(&mut self) -> bool {
        let Some(show) = self.stop_slideshow_quietly() else {
            return false;
        };
        self.library.fullscreen_restore |= show.entered_fullscreen;
        true
    }

    /// End the show: back to the grid, on the photo it ended on, and out
    /// of fullscreen if the show put the window there.
    pub(crate) fn stop_slideshow(&mut self, cx: &mut Context<Self>) {
        let last = self.slideshow_photo();
        if !self.drop_slideshow() {
            return;
        }
        if let Some(path) = last.filter(|p| self.library.entry_of(p).is_some()) {
            self.library.select_single(path);
            self.library.grid.reveal = true;
        }
        cx.notify();
    }

    fn ensure_slideshow_ticker(&mut self, cx: &mut Context<Self>) {
        let Some(show) = &mut self.library.slideshow else {
            return;
        };
        if show.ticking {
            return;
        }
        show.ticking = true;
        let alive = show.alive.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(FRAME).await;
            if !alive.load(Ordering::Acquire) {
                break;
            }
            let keep = this
                .update(cx, |ws, cx| ws.slideshow_tick(cx))
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }

    /// One look at the clock: move on when the slide has had its time
    /// and the next one is ready, keep the decodes ahead of the show,
    /// and repaint while something moves. Returns whether to keep
    /// ticking.
    fn slideshow_tick(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(show) = &mut self.library.slideshow else {
            return false;
        };
        let now = Instant::now();
        let mut repaint = show.animating();
        // A slide's time starts when it can be seen, not when it was
        // asked for: a slow decode must not eat its screen time.
        if !show.current_ready() && show.paused_at.is_none() {
            show.shown_at = now;
        }
        if show.leaving.is_some() && show.elapsed() >= show.settings.fade_seconds() {
            show.leaving = None;
            repaint = true;
        }
        if show.controls_until.is_some_and(|until| now >= until) {
            show.controls_until = None;
            repaint = true;
        }
        if show.flash.as_ref().is_some_and(|(_, at)| now >= *at) {
            show.flash = None;
            repaint = true;
        }
        let due = show.paused_at.is_none() && !show.ended && show.current_ready() && {
            match show.current() {
                Some(item) if schist_gallery::is_video(&show.items[item]) => show
                    .video
                    .as_ref()
                    .is_some_and(|v| v.item == item && v.done),
                Some(_) => show.elapsed() >= show.settings.seconds,
                None => false,
            }
        };
        if due {
            match show.playlist.peek(1) {
                None => {
                    // The end of a show that does not loop: the last
                    // photo stays, saying so.
                    show.ended = true;
                    show.controls_until = Some(now + CONTROLS_LINGER);
                    repaint = true;
                }
                Some(next) => {
                    let next_ready = schist_gallery::is_video(&show.items[next])
                        || matches!(show.slides.get(&next), Some(Slot::Ready(_) | Slot::Failed));
                    if next_ready {
                        self.slideshow_step(1, cx);
                        return true;
                    }
                }
            }
        }
        self.slideshow_preload(cx);
        self.slideshow_video(cx);
        if repaint {
            cx.notify();
        }
        true
    }

    /// Forward (1) or back (-1) one slide, crossfading per the options.
    pub(super) fn slideshow_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(show) = &mut self.library.slideshow else {
            return;
        };
        let from = show.current();
        let t = (show.elapsed() / show.settings.seconds).min(1.0);
        let zoom_in = show.zoom_in();
        let moved = if delta >= 0 {
            show.playlist.advance()
        } else {
            show.playlist.back()
        };
        if !moved {
            if delta >= 0 && !show.settings.repeat {
                show.ended = true;
                show.controls_until = Some(Instant::now() + CONTROLS_LINGER);
            }
            cx.notify();
            return;
        }
        show.ended = false;
        show.leaving = match (from, show.settings.fade_seconds() > 0.0) {
            (Some(item), true) if !schist_gallery::is_video(&show.items[item]) => {
                Some(Leaving { item, t, zoom_in })
            }
            _ => None,
        };
        show.shown_at = Instant::now();
        if show.paused_at.is_some() {
            show.paused_at = Some(show.shown_at);
        }
        if let Some(video) = show.video.take() {
            video.job.cancel();
            if let Some((frame, _)) = video.frame {
                self.retired_images.push(frame);
            }
        }
        self.slideshow_preload(cx);
        self.slideshow_video(cx);
        cx.notify();
    }

    /// Pause, or carry on from where the pause was.
    pub(super) fn slideshow_toggle_pause(&mut self, cx: &mut Context<Self>) {
        let Some(show) = &mut self.library.slideshow else {
            return;
        };
        let now = Instant::now();
        if show.ended {
            // Play at the end of a finished show starts it again.
            show.ended = false;
            show.paused_at = None;
            show.leaving = None;
            show.playlist.rewind();
            show.shown_at = now;
            if let Some(video) = show.video.take() {
                video.job.cancel();
                if let Some((frame, _)) = video.frame {
                    self.retired_images.push(frame);
                }
            }
            self.slideshow_preload(cx);
            self.slideshow_video(cx);
            cx.notify();
            return;
        }
        match show.paused_at.take() {
            Some(paused) => {
                show.shown_at += now.saturating_duration_since(paused);
            }
            None => {
                show.paused_at = Some(now);
                show.controls_until = Some(now + CONTROLS_LINGER);
                // A video stops decoding while paused and resumes from
                // the frame it showed.
                if let Some(video) = &mut show.video {
                    video.job.cancel();
                }
            }
        }
        self.slideshow_video(cx);
        cx.notify();
    }

    /// Keep the slides in the preload window decoded and nothing else:
    /// the current one first, the next two, the one before. Everything
    /// outside the window (save a slide still fading out) goes back.
    fn slideshow_preload(&mut self, cx: &mut Context<Self>) {
        let Some(show) = &mut self.library.slideshow else {
            return;
        };
        if show.edge == 0 {
            return;
        }
        let window = show.playlist.window(PRELOAD_AHEAD, KEEP_BEHIND);
        let leaving = show.leaving.as_ref().map(|l| l.item);
        let stale: Vec<usize> = show
            .slides
            .keys()
            .copied()
            .filter(|i| !window.contains(i) && Some(*i) != leaving)
            .collect();
        for item in stale {
            if let Some(Slot::Ready(slide)) = show.slides.remove(&item) {
                self.retired_images.push(slide.render);
            }
        }
        let wanted: Vec<(usize, PathBuf)> = window
            .into_iter()
            .filter(|i| !show.slides.contains_key(i))
            .map(|i| (i, show.items[i].clone()))
            .filter(|(_, p)| !schist_gallery::is_video(p))
            .collect();
        let edge = show.edge;
        let alive = show.alive.clone();
        for (item, _) in &wanted {
            show.slides.insert(*item, Slot::Loading);
        }
        for (item, path) in wanted {
            let edited = self.library.entry_of(&path).is_some_and(|e| e.edited);
            let source = schist_gallery::thumb_source(&path, edited);
            let faces: Vec<FaceRect> = self
                .library
                .detected_faces(&path)
                .map(|faces| faces.iter().map(|f| f.rect).collect())
                .unwrap_or_default();
            let focus = show::subject_focus(&faces);
            let alive = alive.clone();
            cx.spawn(async move |this, cx| {
                let decoded = cx
                    .background_executor()
                    .spawn(async move {
                        let picture = schist_preview::render_file(&source, edge)
                            .map_err(|err| {
                                log::warn!(
                                    "slideshow decode failed for {}: {err:#}",
                                    source.display()
                                );
                            })
                            .ok()
                            .map(|p| (p.width, p.height, p.rgba));
                        let caption = schist_gallery::xmp::read(&path)
                            .ok()
                            .map(|m| m.caption.trim().to_string())
                            .filter(|c| !c.is_empty());
                        let title = schist_gallery::photo_display_name(&path);
                        (picture, caption, title)
                    })
                    .await;
                if !alive.load(Ordering::Acquire) {
                    return;
                }
                this.update(cx, |ws, cx| {
                    let Some(show) = ws
                        .library
                        .slideshow
                        .as_mut()
                        .filter(|s| Arc::ptr_eq(&s.alive, &alive))
                    else {
                        return;
                    };
                    // Moved out of the window while it decoded.
                    if !matches!(show.slides.get(&item), Some(Slot::Loading)) {
                        return;
                    }
                    let (picture, caption, title) = decoded;
                    let slot = picture
                        .and_then(|(w, h, rgba)| {
                            super::library::rgba_to_render_image(w, h, rgba).map(|render| {
                                Slot::Ready(SlideImage {
                                    render,
                                    size: (w as f32, h as f32),
                                    focus,
                                    title,
                                    caption,
                                })
                            })
                        })
                        .unwrap_or(Slot::Failed);
                    show.slides.insert(item, slot);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// Start (or resume) the current slide's video when it is one and
    /// the show is running.
    fn slideshow_video(&mut self, cx: &mut Context<Self>) {
        let Some(show) = &mut self.library.slideshow else {
            return;
        };
        let Some(item) = show.current() else {
            return;
        };
        let path = show.items[item].clone();
        if !schist_gallery::is_video(&path) || show.paused_at.is_some() {
            return;
        }
        let start = match &show.video {
            Some(video) if video.item == item => {
                if video.done || !video.job.cancelled() {
                    return;
                }
                video.time
            }
            _ => 0.0,
        };
        let job = Arc::new(crate::video::Job::default());
        let previous_frame = show
            .video
            .take()
            .filter(|v| v.item == item)
            .and_then(|v| v.frame);
        show.video = Some(SlideVideo {
            item,
            job: job.clone(),
            frame: previous_frame,
            time: start,
            done: false,
        });
        let (mut tx, mut rx) =
            futures::channel::mpsc::channel::<anyhow::Result<crate::video::Frame>>(1);
        let worker = job.clone();
        cx.background_executor()
            .spawn(async move {
                use futures::SinkExt;
                let result = (|| -> anyhow::Result<()> {
                    let mut decoder = crate::video::Decoder::open(
                        &path,
                        start,
                        None,
                        crate::video::PREVIEW_EDGE,
                        worker.clone(),
                    )?;
                    while let Some(frame) = decoder.next_frame()? {
                        if worker.cancelled() {
                            break;
                        }
                        if futures::executor::block_on(tx.send(Ok(frame))).is_err() {
                            break;
                        }
                    }
                    Ok(())
                })();
                if let Err(err) = result {
                    let _ = tx.send(Err(err)).await;
                }
            })
            .detach();
        cx.spawn(async move |this, cx| {
            use futures::StreamExt;
            let mut clock: Option<(Instant, f64)> = None;
            let mut failed = false;
            while let Some(result) = rx.next().await {
                if job.cancelled() {
                    break;
                }
                let frame = match result {
                    Ok(frame) => frame,
                    Err(err) => {
                        log::warn!("slideshow video failed: {err:#}");
                        failed = true;
                        break;
                    }
                };
                let (began, first) = *clock.get_or_insert((Instant::now(), frame.time));
                let wait = (frame.time - first - began.elapsed().as_secs_f64()).max(0.0);
                let until = Instant::now() + Duration::from_secs_f64(wait);
                while Instant::now() < until && !job.cancelled() {
                    cx.background_executor()
                        .timer(Duration::from_millis(10))
                        .await;
                }
                if job.cancelled() {
                    break;
                }
                let keep = this
                    .update(cx, |ws, cx| {
                        let Some(video) = ws
                            .library
                            .slideshow
                            .as_mut()
                            .and_then(|s| s.video.as_mut())
                            .filter(|v| Arc::ptr_eq(&v.job, &job))
                        else {
                            return false;
                        };
                        video.time = frame.time;
                        let size = (frame.width as f32, frame.height as f32);
                        if let Some(image) = super::library::rgba_to_render_image(
                            frame.width,
                            frame.height,
                            frame.rgba,
                        ) {
                            if let Some((old, _)) = video.frame.replace((image, size)) {
                                ws.retired_images.push(old);
                            }
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    break;
                }
            }
            // Played to the end (or could not play at all): the slide is
            // over. A cancelled job was a pause or a skip, not an end.
            if !job.cancelled() || failed {
                this.update(cx, |ws, cx| {
                    if let Some(video) = ws
                        .library
                        .slideshow
                        .as_mut()
                        .and_then(|s| s.video.as_mut())
                        .filter(|v| Arc::ptr_eq(&v.job, &job))
                    {
                        // Over: the clock moves on (a clip that never
                        // produced a frame says so until it does).
                        video.done = true;
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    /// A key while the show is up. Everything is the show's: arrows and
    /// space drive it, the culling keys decide on the photo on screen,
    /// and nothing reaches the grid underneath. Modified keys (⌘Q and
    /// the like) are left for the app.
    pub(super) fn slideshow_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.library.slideshow.is_none() {
            return false;
        }
        let mods = ev.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt || mods.function {
            return false;
        }
        let key = ev.keystroke.key.as_str();
        match key {
            "space" => self.slideshow_toggle_pause(cx),
            "right" | "down" | "pagedown" => self.slideshow_step(1, cx),
            "left" | "up" | "pageup" => self.slideshow_step(-1, cx),
            "escape" => self.stop_slideshow(cx),
            "i" => {
                if let Some(show) = &mut self.library.slideshow {
                    show.captions = !show.captions;
                }
                cx.notify();
            }
            _ => {
                if let Some(edit) = schist_gallery_ui::culling::shortcut(key) {
                    self.apply_culling(edit, cx);
                    self.slideshow_flash_decision();
                    cx.notify();
                }
            }
        }
        true
    }

    /// Say what a culling key just did, when the caption is not up to
    /// say it.
    fn slideshow_flash_decision(&mut self) {
        let Some(path) = self.slideshow_photo() else {
            return;
        };
        let culling = self.library.culling_of(&path);
        let Some(show) = &mut self.library.slideshow else {
            return;
        };
        if show.captions {
            return;
        }
        show.flash = Some((
            decision_text(culling).unwrap_or_else(|| "☆".into()),
            Instant::now() + FLASH_LINGER,
        ));
    }

    /// The pointer moved over the show: bring the controls back.
    fn slideshow_poke(&mut self, cx: &mut Context<Self>) {
        if let Some(show) = &mut self.library.slideshow {
            let was = show.controls_until.is_some();
            show.controls_until = Some(Instant::now() + CONTROLS_LINGER);
            if !was {
                cx.notify();
            }
        }
    }
}

/// "★★★★☆ · Pick · Red", or `None` for a photo with no decisions.
fn decision_text(culling: schist_gallery::culling::PhotoCulling) -> Option<String> {
    use schist_gallery::culling::{ColourLabel, CullFlag};
    if culling == Default::default() {
        return None;
    }
    let mut parts = Vec::new();
    if culling.rating > 0 {
        let stars = culling.rating.min(5) as usize;
        parts.push(format!("{}{}", "★".repeat(stars), "☆".repeat(5 - stars)));
    }
    if culling.flag != CullFlag::None {
        parts.push(schist_gallery_ui::culling::flag_name(culling.flag).to_string());
    }
    if culling.label != ColourLabel::None {
        parts.push(schist_gallery_ui::culling::colour_name(culling.label).to_string());
    }
    Some(parts.join(" · "))
}

/// The slideshow, filling the window: the photo (or two, crossfading),
/// the caption, and the controls while the pointer is about.
pub(super) fn render(
    ws: &mut Workspace,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let viewport = window.viewport_size();
    let view = (f32::from(viewport.width), f32::from(viewport.height));
    let scale = window.scale_factor();
    let touch = crate::ui::touch();
    let caption_photo = ws.slideshow_photo();
    let caption_lines = caption_photo
        .as_ref()
        .map(|path| caption_lines(ws, path))
        .unwrap_or_default();
    let Some(show) = &mut ws.library.slideshow else {
        return div().into_any_element();
    };
    if show.want_fullscreen {
        show.want_fullscreen = false;
        if !window.is_fullscreen() {
            window.toggle_fullscreen();
            show.entered_fullscreen = true;
        }
    }
    let edge = show::display_edge(view.0 * scale, view.1 * scale);
    let measured = show.edge == 0;
    if measured || edge > show.edge {
        // First frame, or a bigger screen than the decodes were made for
        // (the window went fullscreen): decode at this size from here on.
        show.edge = edge;
    }
    let elapsed = show.elapsed();
    let fade = show.settings.fade_seconds();
    let ken_burns = show.settings.transition == Transition::KenBurns;
    let seconds = show.settings.seconds;
    let frame_for = |slide: &SlideImage, t: f32, zoom_in: bool| {
        if ken_burns {
            show::ken_burns(slide.size, view, slide.focus, t, zoom_in)
        } else {
            show::fit(slide.size, view)
        }
    };
    let mut layers: Vec<gpui::AnyElement> = Vec::new();
    let current = show.current();
    let current_ready = show.current_ready();
    // The slide fading out sits underneath.
    if let Some(leaving) = &show.leaving {
        if let Some(Slot::Ready(slide)) = show.slides.get(&leaving.item) {
            let k = if current_ready && fade > 0.0 {
                (elapsed / fade).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let f = frame_for(slide, leaving.t, leaving.zoom_in);
            layers.push(picture(slide.render.clone(), f, 1.0 - k));
        }
    }
    // The caption block's first lines: the name, then the XMP caption.
    let mut caption_head: Vec<String> = Vec::new();
    let mut failed = false;
    if let Some(item) = current {
        if schist_gallery::is_video(&show.items[item]) {
            caption_head.push(
                show.items[item]
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
            let video = show.video.as_ref().filter(|v| v.item == item);
            match video.and_then(|v| v.frame.clone()) {
                Some((frame, size)) => layers.push(picture(frame, show::fit(size, view), 1.0)),
                None => failed = video.is_some_and(|v| v.done),
            }
        } else {
            match show.slides.get(&item) {
                Some(Slot::Ready(slide)) => {
                    let opacity = if show.leaving.is_some() && fade > 0.0 {
                        (elapsed / fade).clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    let t = (elapsed / seconds).clamp(0.0, 1.0);
                    let zoom_in = show.zoom_in();
                    layers.push(picture(
                        slide.render.clone(),
                        frame_for(slide, t, zoom_in),
                        opacity,
                    ));
                    caption_head.push(slide.title.clone());
                    caption_head.extend(slide.caption.clone());
                }
                Some(Slot::Failed) => failed = true,
                _ => {}
            }
        }
    }
    let loading = !current_ready && show.leaving.is_none();
    let paused = show.paused_at.is_some();
    let ended = show.ended;
    let controls = touch || paused || ended || show.controls_until.is_some();
    let position = tf!(
        "slideshow.position",
        n = show.playlist.position() + 1,
        of = show.playlist.len()
    );
    let flash = show.flash.as_ref().map(|(text, _)| text.clone());
    let captions = show.captions;

    let mut root = div()
        .id("slideshow")
        .size_full()
        .relative()
        .overflow_hidden()
        .bg(gpui::rgb(0x000000))
        .text_color(gpui::rgb(0xFFFFFF))
        .track_focus(&ws.focus)
        .on_key_down(cx.listener(|ws, ev: &gpui::KeyDownEvent, window, cx| {
            if ws.modal_key(ev, window, cx) {
                return;
            }
            if ws.slideshow_key(ev, cx) {
                cx.stop_propagation();
            }
        }))
        .on_mouse_move(cx.listener(|ws, _ev: &gpui::MouseMoveEvent, _w, cx| {
            ws.slideshow_poke(cx);
        }))
        .children(layers);
    if loading || failed {
        root = root.child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(14.0))
                .text_color(gpui::rgb(0xBBBBBB))
                .child(if failed {
                    t("slideshow.failed")
                } else {
                    t("slideshow.loading")
                }),
        );
    }
    if captions && !caption_head.is_empty() {
        let mut lines = caption_head;
        lines.extend(caption_lines);
        root = root.child(
            div()
                .absolute()
                .left(px(24.0))
                .bottom(px(if controls { 72.0 } else { 24.0 }))
                .max_w(px((view.0 - 48.0).max(120.0)))
                .flex()
                .flex_col()
                .gap_1()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(gpui::rgba(0x0000_0099))
                .children(lines.into_iter().enumerate().map(|(i, line)| {
                    div()
                        .text_size(px(if i == 0 { 15.0 } else { 12.0 }))
                        .text_color(gpui::rgb(if i == 0 { 0xFFFFFF } else { 0xDDDDDD }))
                        .child(line)
                })),
        );
    }
    if let Some(flash) = flash {
        root = root.child(
            div()
                .absolute()
                .top(px(24.0))
                .left_0()
                .w_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .px_3()
                        .py_1()
                        .rounded_md()
                        .bg(gpui::rgba(0x0000_00AA))
                        .text_size(px(16.0))
                        .child(flash),
                ),
        );
    }
    if controls {
        let button = |label: &'static str,
                      action: fn(&mut Workspace, &mut Context<Workspace>),
                      cx: &mut Context<Workspace>| {
            gallery_button(label, false, move |ws, _w, cx| action(ws, cx), cx)
                .h(px(if touch { 44.0 } else { 28.0 }))
                .text_size(px(if touch { 14.0 } else { 12.0 }))
        };
        let status = if ended {
            Some(t("slideshow.ended"))
        } else if paused {
            Some(t("slideshow.paused"))
        } else {
            None
        };
        root = root.child(
            div()
                .absolute()
                .left_0()
                .bottom_0()
                .w_full()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .justify_center()
                .gap_2()
                .p_3()
                .bg(gpui::rgba(0x0000_0080))
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(gpui::rgb(0xDDDDDD))
                        .child(position),
                )
                .children(status.map(|s| {
                    div()
                        .text_size(px(12.0))
                        .text_color(gpui::rgb(pal().green))
                        .child(s)
                }))
                .child(button(
                    t("slideshow.previous"),
                    |ws, cx| ws.slideshow_step(-1, cx),
                    cx,
                ))
                .child(button(
                    if paused || ended {
                        t("slideshow.resume")
                    } else {
                        t("slideshow.pause")
                    },
                    |ws, cx| ws.slideshow_toggle_pause(cx),
                    cx,
                ))
                .child(button(
                    t("slideshow.next"),
                    |ws, cx| ws.slideshow_step(1, cx),
                    cx,
                ))
                .child(button(
                    t("slideshow.info"),
                    |ws, cx| {
                        if let Some(show) = &mut ws.library.slideshow {
                            show.captions = !show.captions;
                        }
                        cx.notify();
                    },
                    cx,
                ))
                .child(button(
                    t("slideshow.exit"),
                    |ws, cx| ws.stop_slideshow(cx),
                    cx,
                ))
                .children((!touch).then(|| {
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .text_size(px(11.0))
                        .text_color(gpui::rgb(0xAAAAAA))
                        .child(t("slideshow.keys"))
                })),
        );
    }
    if measured {
        // The screen is measured now; the decodes can start.
        cx.defer_in(window, |ws, _w, cx| ws.slideshow_preload(cx));
    }
    root.into_any_element()
}

/// One picture layer at its framing.
fn picture(render: Arc<RenderImage>, f: show::Framing, opacity: f32) -> gpui::AnyElement {
    img(render)
        .absolute()
        .left(px(f.left))
        .top(px(f.top))
        .w(px(f.width))
        .h(px(f.height))
        .opacity(opacity)
        .into_any_element()
}

/// The caption block's lines below the name and caption: the date,
/// the place, and the decisions on the photo.
fn caption_lines(ws: &Workspace, path: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    let mut detail = Vec::new();
    if let Some(entry) = ws.library.entry_of(path) {
        let taken = ws.library.taken_of(entry);
        detail.push(taken.get(..16).unwrap_or(&taken).to_string());
    }
    if let Some(place) = ws.library.place_of(path) {
        detail.push(place);
    }
    if !detail.is_empty() {
        lines.push(detail.join(" · "));
    }
    if let Some(decisions) = decision_text(ws.library.culling_of(path)) {
        lines.push(decisions);
    }
    lines
}

/// The options asked before a show starts. Remembered for next time.
pub(crate) fn slideshow_dialog(
    ws: &mut Workspace,
    photos: usize,
    settings: SlideshowSettings,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let edit = |f: fn(&mut SlideshowSettings)| {
        move |ws: &mut Workspace, _cx: &mut Context<Workspace>| {
            ws.update_modal(|m| {
                if let Modal::Slideshow { settings, .. } = m {
                    f(settings);
                }
            });
        }
    };
    const SECONDS: [u32; 8] = [2, 3, 5, 8, 10, 15, 30, 60];
    let seconds = settings.seconds.round() as u32;
    let transition_name = |t: Transition| match t {
        Transition::Cut => schist_i18n::t("slideshow.transition.cut"),
        Transition::Crossfade => schist_i18n::t("slideshow.transition.crossfade"),
        Transition::KenBurns => schist_i18n::t("slideshow.transition.ken_burns"),
    };
    let video_name = |v: VideoSlides| match v {
        VideoSlides::Skip => schist_i18n::t("slideshow.videos.skip"),
        VideoSlides::Play => schist_i18n::t("slideshow.videos.play"),
    };
    let body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_size(px(12.0))
                .child(tn("slideshow.n_slides", photos as u64)),
        )
        .child(crate::ui::field_row(
            t("slideshow.seconds"),
            crate::ui::dropdown(
                &ws.dropdown,
                crate::ui::Dropdown {
                    popup: Popup::Field("slideshow-seconds"),
                    is_open: ws.open_popup == Some(Popup::Field("slideshow-seconds")),
                    current: seconds,
                    label: tn("slideshow.seconds_n", seconds as u64).into(),
                    width: 220.0,
                    options: SECONDS
                        .iter()
                        .map(|s| (SharedString::from(tn("slideshow.seconds_n", *s as u64)), *s))
                        .collect(),
                },
                |ws, value, _cx| {
                    ws.update_modal(|m| {
                        if let Modal::Slideshow { settings, .. } = m {
                            settings.seconds = value as f32;
                        }
                    })
                },
                cx,
            ),
        ))
        .child(crate::ui::field_row(
            t("slideshow.transition"),
            crate::ui::dropdown(
                &ws.dropdown,
                crate::ui::Dropdown {
                    popup: Popup::Field("slideshow-transition"),
                    is_open: ws.open_popup == Some(Popup::Field("slideshow-transition")),
                    current: settings.transition,
                    label: transition_name(settings.transition).into(),
                    width: 220.0,
                    options: Transition::ALL
                        .iter()
                        .map(|tr| (SharedString::from(transition_name(*tr)), *tr))
                        .collect(),
                },
                |ws, value, _cx| {
                    ws.update_modal(|m| {
                        if let Modal::Slideshow { settings, .. } = m {
                            settings.transition = value;
                        }
                    })
                },
                cx,
            ),
        ))
        .children((settings.transition == Transition::KenBurns).then(|| {
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(crate::ui::palette().text_dim))
                .child(t("slideshow.ken_burns_help"))
        }))
        .child(crate::ui::field_row(
            t("slideshow.videos"),
            crate::ui::dropdown(
                &ws.dropdown,
                crate::ui::Dropdown {
                    popup: Popup::Field("slideshow-videos"),
                    is_open: ws.open_popup == Some(Popup::Field("slideshow-videos")),
                    current: settings.videos,
                    label: video_name(settings.videos).into(),
                    width: 220.0,
                    options: [VideoSlides::Skip, VideoSlides::Play]
                        .iter()
                        .map(|v| (SharedString::from(video_name(*v)), *v))
                        .collect(),
                },
                |ws, value, _cx| {
                    ws.update_modal(|m| {
                        if let Modal::Slideshow { settings, .. } = m {
                            settings.videos = value;
                        }
                    })
                },
                cx,
            ),
        ))
        .child(crate::ui::checkbox(
            t("slideshow.shuffle"),
            settings.shuffle,
            edit(|s| s.shuffle = !s.shuffle),
            cx,
        ))
        .child(crate::ui::checkbox(
            t("slideshow.repeat"),
            settings.repeat,
            edit(|s| s.repeat = !s.repeat),
            cx,
        ))
        .child(crate::ui::checkbox(
            t("slideshow.captions"),
            settings.captions,
            edit(|s| s.captions = !s.captions),
            cx,
        ))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(gpui::rgb(crate::ui::palette().text_dim))
                .child(t("slideshow.keys")),
        );
    let actions = div()
        .flex()
        .flex_row()
        .gap_2()
        .child(crate::ui::button(
            t("common.cancel"),
            false,
            |ws, _w, cx| ws.close_modal(cx),
            cx,
        ))
        .child(crate::ui::button(
            t("slideshow.start"),
            true,
            |ws, _w, cx| {
                let Some(Modal::Slideshow {
                    photos,
                    start,
                    settings,
                }) = ws.modal.clone()
                else {
                    return;
                };
                ws.close_modal(cx);
                ws.library.slideshow_settings = settings.clone().sanitized();
                ws.library.save();
                ws.start_slideshow(photos, start, settings, cx);
            },
            cx,
        ));
    crate::ui::modal_frame(t("slideshow.title"), 460.0, body, actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_gallery::culling::{ColourLabel, CullFlag, PhotoCulling};

    #[test]
    fn decisions_read_as_stars_flag_and_label() {
        assert_eq!(decision_text(PhotoCulling::default()), None);
        let text = decision_text(PhotoCulling {
            rating: 3,
            flag: CullFlag::Pick,
            label: ColourLabel::None,
        })
        .unwrap();
        assert!(text.starts_with("★★★☆☆ · "), "{text}");
        let label_only = decision_text(PhotoCulling {
            rating: 0,
            flag: CullFlag::None,
            label: ColourLabel::Red,
        })
        .unwrap();
        assert!(!label_only.contains('★'));
    }
}
