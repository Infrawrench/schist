//! The slideshow's arithmetic: the order slides play in, which of them
//! to have decoded ahead of time, and where a Ken Burns pan and zoom
//! puts the picture at each moment. No pixels here — the app decodes
//! and draws; this decides what and where, and is tested on its own.

use crate::people::FaceRect;

/// What the slideshow does between two slides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    Cut,
    #[default]
    Crossfade,
    /// A slow pan and zoom across each photo, towards the faces in it
    /// when People found any, crossfading between slides.
    KenBurns,
}

impl Transition {
    pub const ALL: [Transition; 3] = [Transition::Cut, Transition::Crossfade, Transition::KenBurns];
}

/// What a video in the selection becomes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoSlides {
    /// Left out of the show.
    #[default]
    Skip,
    /// Played through, silently, as its own slide.
    Play,
}

/// The slideshow's options, remembered between shows in `library.json`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SlideshowSettings {
    /// How long each photo stays up, in seconds.
    pub seconds: f32,
    pub transition: Transition,
    pub shuffle: bool,
    /// Start again from the top after the last slide.
    pub repeat: bool,
    /// Title, caption, date, place and rating over the photo.
    pub captions: bool,
    pub videos: VideoSlides,
}

impl Default for SlideshowSettings {
    fn default() -> Self {
        SlideshowSettings {
            seconds: 5.0,
            transition: Transition::Crossfade,
            shuffle: false,
            repeat: false,
            captions: true,
            videos: VideoSlides::Skip,
        }
    }
}

/// The shortest and longest a slide may be set to stay up.
pub const MIN_SECONDS: f32 = 1.0;
pub const MAX_SECONDS: f32 = 60.0;
/// How long a crossfade takes, capped to a third of the slide so a
/// one-second show still spends most of its time showing.
pub const FADE_SECONDS: f32 = 0.8;

impl SlideshowSettings {
    /// The settings with anything out of range brought back in — a
    /// hand-edited file must not ask for a zero-second slide.
    pub fn sanitized(mut self) -> Self {
        if !self.seconds.is_finite() {
            self.seconds = SlideshowSettings::default().seconds;
        }
        self.seconds = self.seconds.clamp(MIN_SECONDS, MAX_SECONDS);
        self
    }

    /// The crossfade's length for these settings; zero for a cut.
    pub fn fade_seconds(&self) -> f32 {
        match self.transition {
            Transition::Cut => 0.0,
            Transition::Crossfade | Transition::KenBurns => FADE_SECONDS.min(self.seconds / 3.0),
        }
    }
}

/// A small, seedable shuffle source (SplitMix64): the show needs a fair
/// order, not cryptography, and a seed makes the order testable.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (n > 0), by rejection so no index is favoured.
    fn below(&mut self, n: usize) -> usize {
        let n = n as u64;
        let zone = u64::MAX - u64::MAX % n;
        loop {
            let v = self.next();
            if v < zone {
                return (v % n) as usize;
            }
        }
    }
}

/// The order a show plays its slides in, and where it is. Slides are
/// indices into the caller's list.
#[derive(Clone, Debug, PartialEq)]
pub struct Playlist {
    order: Vec<usize>,
    at: usize,
    repeat: bool,
    shuffle: bool,
    rng: u64,
}

impl Playlist {
    /// A show over `len` slides starting with slide `start`. Shuffled,
    /// the starting slide still comes first — "play from this photo"
    /// should open on this photo — and the rest follow in random order.
    pub fn new(len: usize, start: usize, shuffle: bool, repeat: bool, seed: u64) -> Playlist {
        let start = start.min(len.saturating_sub(1));
        let mut list = Playlist {
            order: Vec::new(),
            at: 0,
            repeat,
            shuffle,
            rng: seed,
        };
        if len == 0 {
            return list;
        }
        if shuffle {
            let rest: Vec<usize> = (0..len).filter(|&i| i != start).collect();
            let rest = list.shuffled(rest);
            list.order = std::iter::once(start).chain(rest).collect();
        } else {
            list.order = (0..len).collect();
            list.at = start;
        }
        list
    }

    fn shuffled(&mut self, mut items: Vec<usize>) -> Vec<usize> {
        let mut rng = SplitMix(self.rng);
        for i in (1..items.len()).rev() {
            items.swap(i, rng.below(i + 1));
        }
        self.rng = rng.0;
        items
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The slide on show.
    pub fn current(&self) -> Option<usize> {
        self.order.get(self.at).copied()
    }

    /// The position in the show, 0-based, for "3 / 40".
    pub fn position(&self) -> usize {
        self.at
    }

    /// The slide `steps` after the current one, following the wrap
    /// when the show repeats; `None` past either end otherwise.
    pub fn peek(&self, steps: isize) -> Option<usize> {
        let len = self.order.len() as isize;
        if len == 0 {
            return None;
        }
        let at = self.at as isize + steps;
        let at = if self.repeat {
            at.rem_euclid(len)
        } else if (0..len).contains(&at) {
            at
        } else {
            return None;
        };
        self.order.get(at as usize).copied()
    }

    /// On to the next slide. At the end a repeating show starts over —
    /// reshuffled when shuffling, never opening on the slide it just
    /// ended with — and a non-repeating one stays put and says so.
    pub fn advance(&mut self) -> bool {
        if self.order.is_empty() {
            return false;
        }
        if self.at + 1 < self.order.len() {
            self.at += 1;
            return true;
        }
        if !self.repeat {
            return false;
        }
        if self.shuffle && self.order.len() > 1 {
            let last = self.order[self.at];
            let mut next = self.shuffled(self.order.clone());
            if next[0] == last {
                let swap = 1 + SplitMix(self.rng).below(next.len() - 1);
                next.swap(0, swap);
            }
            self.order = next;
        }
        self.at = 0;
        true
    }

    /// Back to the first slide, for playing a finished show again.
    pub fn rewind(&mut self) {
        self.at = 0;
    }

    /// Back one slide; a repeating show wraps to its end.
    pub fn back(&mut self) -> bool {
        if self.order.is_empty() {
            return false;
        }
        if self.at > 0 {
            self.at -= 1;
            true
        } else if self.repeat {
            self.at = self.order.len() - 1;
            true
        } else {
            false
        }
    }

    /// Which slides to have decoded, most wanted first: the current
    /// one, then `ahead` coming up, then `behind` gone by (for the
    /// back arrow). Each once, so a short repeating show does not ask
    /// for a slide twice.
    pub fn window(&self, ahead: usize, behind: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let steps = std::iter::once(0)
            .chain((1..=ahead).map(|s| s as isize))
            .chain((1..=behind).map(|s| -(s as isize)));
        for step in steps {
            if let Some(slide) = self.peek(step) {
                if !out.contains(&slide) {
                    out.push(slide);
                }
            }
        }
        out
    }
}

/// How many slides ahead the show decodes, and how many it keeps
/// behind — enough that the next slide is always ready and the back
/// arrow is instant, few enough that a show of a thousand photos holds
/// four of them at display size.
pub const PRELOAD_AHEAD: usize = 2;
pub const KEEP_BEHIND: usize = 1;

/// The longest edge to decode a slide at for a screen: the screen's
/// own longest edge in device pixels, within sane bounds. Never the
/// original's full resolution — a 50-megapixel raw shown on a laptop
/// would be 200 MB of pixels for nothing.
pub fn display_edge(width_px: f32, height_px: f32) -> u32 {
    let edge = width_px.max(height_px);
    if !width_px.is_finite() || !height_px.is_finite() {
        return 1920;
    }
    (edge.ceil() as u32).clamp(640, 4096)
}

/// Where a picture is drawn: its top-left corner and size in the
/// view's coordinates. Parts may hang off the edges; the view clips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Framing {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

/// The picture fitted whole inside the view, centred — the letterboxed
/// "contain" fit.
pub fn fit(image: (f32, f32), view: (f32, f32)) -> Framing {
    let (iw, ih) = (image.0.max(1.0), image.1.max(1.0));
    let scale = (view.0 / iw).min(view.1 / ih).max(0.0);
    let (width, height) = (iw * scale, ih * scale);
    Framing {
        left: (view.0 - width) / 2.0,
        top: (view.1 - height) / 2.0,
        width,
        height,
    }
}

/// How far a Ken Burns slide zooms over its run.
pub const KEN_BURNS_ZOOM: f32 = 1.2;

/// Where a Ken Burns slide puts the picture at `t` (0 at the start of
/// the slide, 1 at its end). The camera moves between the whole
/// picture fitted and centred and a closer view centred on `focus` (a
/// point in the picture, as fractions) — in on even slides, out on odd
/// ones, so a show does not only ever push in. The picture is kept
/// against the view's edges along any axis where it overflows them, so
/// zooming towards a face at the very edge never drags in a band of
/// background; along an axis where it still fits it stays centred.
pub fn ken_burns(
    image: (f32, f32),
    view: (f32, f32),
    focus: (f32, f32),
    t: f32,
    zoom_in: bool,
) -> Framing {
    let base = fit(image, view);
    let t = t.clamp(0.0, 1.0);
    // Ease in and out: a camera that starts and stops abruptly looks
    // like a slideshow, one that glides looks like a film.
    let eased = t * t * (3.0 - 2.0 * t);
    let p = if zoom_in { eased } else { 1.0 - eased };
    let zoom = 1.0 + (KEN_BURNS_ZOOM - 1.0) * p;
    let width = base.width * zoom;
    let height = base.height * zoom;
    let focus = (focus.0.clamp(0.0, 1.0), focus.1.clamp(0.0, 1.0));
    let target = (0.5 + (focus.0 - 0.5) * p, 0.5 + (focus.1 - 0.5) * p);
    let place = |view: f32, size: f32, at: f32| -> f32 {
        if size <= view {
            (view - size) / 2.0
        } else {
            (view / 2.0 - at * size).clamp(view - size, 0.0)
        }
    };
    Framing {
        left: place(view.0, width, target.0),
        top: place(view.1, height, target.1),
        width,
        height,
    }
}

/// The point a Ken Burns slide moves towards: the middle of the faces
/// People found in it, bigger faces counting for more, or the centre
/// of the picture when there are none.
pub fn subject_focus(faces: &[FaceRect]) -> (f32, f32) {
    let mut weight = 0.0f32;
    let (mut x, mut y) = (0.0f32, 0.0f32);
    for face in faces {
        let area = (face.w * face.h).max(0.0);
        if area <= 0.0 || !area.is_finite() {
            continue;
        }
        x += (face.x + face.w / 2.0) * area;
        y += (face.y + face.h / 2.0) * area;
        weight += area;
    }
    if weight > 0.0 {
        ((x / weight).clamp(0.0, 1.0), (y / weight).clamp(0.0, 1.0))
    } else {
        (0.5, 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walk(list: &mut Playlist, n: usize) -> Vec<usize> {
        let mut out = vec![list.current().unwrap()];
        for _ in 1..n {
            if !list.advance() {
                break;
            }
            out.push(list.current().unwrap());
        }
        out
    }

    #[test]
    fn in_order_from_the_chosen_slide_and_stops_at_the_end() {
        let mut list = Playlist::new(4, 2, false, false, 1);
        assert_eq!(walk(&mut list, 10), vec![2, 3]);
        assert!(!list.advance());
        assert_eq!(list.current(), Some(3));
        assert!(list.back());
        assert_eq!(list.current(), Some(2));
        let mut first = Playlist::new(4, 0, false, false, 1);
        assert!(!first.back());
        assert_eq!(first.current(), Some(0));
        // A finished show plays again from its first slide.
        list.rewind();
        assert_eq!(list.current(), Some(0));
        assert_eq!(list.position(), 0);
    }

    #[test]
    fn a_repeating_show_wraps_both_ways() {
        let mut list = Playlist::new(3, 0, false, true, 1);
        assert_eq!(walk(&mut list, 7), vec![0, 1, 2, 0, 1, 2, 0]);
        let mut list = Playlist::new(3, 0, false, true, 1);
        assert!(list.back());
        assert_eq!(list.current(), Some(2));
    }

    #[test]
    fn shuffle_plays_everything_once_starting_on_the_chosen_slide() {
        for seed in 0..50u64 {
            let mut list = Playlist::new(20, 7, true, false, seed);
            let played = walk(&mut list, 100);
            assert_eq!(played[0], 7);
            let mut sorted = played.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..20).collect::<Vec<_>>());
        }
        // Different seeds, different orders; the same seed, the same one.
        let a = walk(&mut Playlist::new(20, 0, true, false, 1), 20);
        let b = walk(&mut Playlist::new(20, 0, true, false, 2), 20);
        let again = walk(&mut Playlist::new(20, 0, true, false, 1), 20);
        assert_ne!(a, b);
        assert_eq!(a, again);
    }

    #[test]
    fn a_repeating_shuffle_reshuffles_without_repeating_the_seam() {
        for seed in 0..50u64 {
            let mut list = Playlist::new(5, 0, true, true, seed);
            let played = walk(&mut list, 15);
            for round in played.chunks(5) {
                let mut sorted = round.to_vec();
                sorted.sort_unstable();
                assert_eq!(sorted, vec![0, 1, 2, 3, 4]);
            }
            assert_ne!(played[4], played[5], "seed {seed}");
            assert_ne!(played[9], played[10], "seed {seed}");
        }
        // One slide repeats itself, which is all it can do.
        let mut one = Playlist::new(1, 0, true, true, 3);
        assert!(one.advance());
        assert_eq!(one.current(), Some(0));
    }

    #[test]
    fn the_preload_window_is_current_then_ahead_then_behind() {
        let list = Playlist::new(10, 5, false, false, 0);
        assert_eq!(list.window(2, 1), vec![5, 6, 7, 4]);
        let end = Playlist::new(10, 9, false, false, 0);
        assert_eq!(end.window(2, 1), vec![9, 8]);
        let wrap = Playlist::new(10, 9, false, true, 0);
        assert_eq!(wrap.window(2, 1), vec![9, 0, 1, 8]);
        // A short repeating show asks for each slide once.
        let short = Playlist::new(2, 0, false, true, 0);
        assert_eq!(short.window(2, 1), vec![0, 1]);
        assert!(Playlist::new(0, 0, true, true, 0).window(2, 1).is_empty());
    }

    #[test]
    fn settings_are_clamped_and_fades_fit_the_slide() {
        let wild = SlideshowSettings {
            seconds: 0.0,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(wild.seconds, MIN_SECONDS);
        let nan = SlideshowSettings {
            seconds: f32::NAN,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(nan.seconds, 5.0);
        let quick = SlideshowSettings {
            seconds: 1.0,
            ..Default::default()
        };
        assert!((quick.fade_seconds() - 1.0 / 3.0).abs() < 1e-6);
        let cut = SlideshowSettings {
            transition: Transition::Cut,
            ..Default::default()
        };
        assert_eq!(cut.fade_seconds(), 0.0);
        let saved: SlideshowSettings = serde_json::from_str(r#"{"shuffle":true}"#).unwrap();
        assert!(saved.shuffle);
        assert_eq!(saved.seconds, 5.0);
    }

    #[test]
    fn display_edge_follows_the_screen_within_bounds() {
        assert_eq!(display_edge(2880.0, 1800.0), 2880);
        assert_eq!(display_edge(320.0, 240.0), 640);
        assert_eq!(display_edge(8000.0, 4000.0), 4096);
        assert_eq!(display_edge(f32::NAN, 1.0), 1920);
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn fit_letterboxes_and_centres() {
        let f = fit((4000.0, 3000.0), (1600.0, 900.0));
        assert!(close(f.height, 900.0) && close(f.width, 1200.0));
        assert!(close(f.left, 200.0) && close(f.top, 0.0));
        let tall = fit((1000.0, 2000.0), (1000.0, 1000.0));
        assert!(close(tall.width, 500.0) && close(tall.left, 250.0));
    }

    #[test]
    fn ken_burns_starts_on_the_fit_and_ends_closer_on_the_subject() {
        let image = (3000.0, 2000.0);
        let view = (1500.0, 1000.0);
        let start = ken_burns(image, view, (0.8, 0.3), 0.0, true);
        assert_eq!(start, fit(image, view));
        let end = ken_burns(image, view, (0.8, 0.3), 1.0, true);
        assert!(close(end.width, 1500.0 * KEN_BURNS_ZOOM));
        // The picture still covers the view: no band of background.
        assert!(end.left <= 0.0 && end.left + end.width >= view.0 - 0.01);
        assert!(end.top <= 0.0 && end.top + end.height >= view.1 - 0.01);
        // The subject has moved towards the middle of the view.
        let subject_x = end.left + 0.8 * end.width;
        let start_x = start.left + 0.8 * start.width;
        assert!((subject_x - view.0 / 2.0).abs() < (start_x - view.0 / 2.0).abs());
        // Zooming out runs the same path backwards.
        assert_eq!(ken_burns(image, view, (0.8, 0.3), 0.0, false), end);
        assert_eq!(ken_burns(image, view, (0.8, 0.3), 1.0, false), start);
    }

    #[test]
    fn ken_burns_never_pulls_an_edge_face_past_the_border() {
        let image = (3000.0, 2000.0);
        let view = (1500.0, 1000.0);
        for t in [0.25, 0.5, 0.75, 1.0] {
            let f = ken_burns(image, view, (1.0, 0.0), t, true);
            assert!(f.left <= 0.01 && f.left + f.width >= view.0 - 0.01, "t {t}");
            assert!(f.top <= 0.01 && f.top + f.height >= view.1 - 0.01, "t {t}");
        }
    }

    #[test]
    fn ken_burns_keeps_a_fitting_axis_centred() {
        // A portrait on a landscape screen: still narrower than the view
        // at full zoom, so it pans only vertically.
        let image = (1000.0, 2000.0);
        let view = (1600.0, 900.0);
        let f = ken_burns(image, view, (0.1, 0.2), 1.0, true);
        assert!(f.width < view.0);
        assert!(close(f.left, (view.0 - f.width) / 2.0));
        assert!(f.top <= 0.0 && f.top + f.height >= view.1 - 0.01);
        // Without faces the camera goes to the middle.
        let centred = ken_burns(image, view, subject_focus(&[]), 1.0, true);
        assert!(close(centred.top, (view.1 - centred.height) / 2.0));
    }

    #[test]
    fn the_subject_is_the_weighted_middle_of_the_faces() {
        assert_eq!(subject_focus(&[]), (0.5, 0.5));
        let small = FaceRect {
            x: 0.0,
            y: 0.0,
            w: 0.1,
            h: 0.1,
        };
        let big = FaceRect {
            x: 0.6,
            y: 0.6,
            w: 0.3,
            h: 0.3,
        };
        let (x, y) = subject_focus(&[small, big]);
        // Nine times the area: the big face pulls the point most of the way.
        assert!(x > 0.65 && x < 0.75 && y > 0.65 && y < 0.75, "{x} {y}");
        let degenerate = FaceRect {
            x: 0.2,
            y: 0.2,
            w: 0.0,
            h: 0.5,
        };
        assert_eq!(subject_focus(&[degenerate]), (0.5, 0.5));
    }
}
