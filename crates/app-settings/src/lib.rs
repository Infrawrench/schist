//! Persisted application preferences and process-wide feature flags.
mod feature_flags;
pub use feature_flags::feature_enabled;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
mod camera;
#[cfg(not(target_arch = "wasm32"))]
pub use camera::*;

/// Where Schist's folder is
#[cfg(not(target_arch = "wasm32"))]
pub fn schist_folder() -> Option<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("schist"))
}

/// Where view preferences are stored.
#[cfg(not(target_arch = "wasm32"))]
fn prefs_path() -> Option<PathBuf> {
    schist_folder().map(|folder| folder.join("preferences.json"))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_view_options() -> ViewOptions {
    prefs_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
pub fn load_view_options() -> ViewOptions {
    schist_app_platform::web::local_get(schist_app_platform::web::PREFS_KEY)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Chrome appearance, following the system unless explicitly selected.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
impl Theme {
    /// The name shown in Preferences, in the user's language.
    pub fn label(self) -> &'static str {
        schist_i18n::t(match self {
            Theme::System => "dialog.prefs.theme_system",
            Theme::Dark => "dialog.prefs.theme_dark",
            Theme::Light => "dialog.prefs.theme_light",
        })
    }
}

/// View toggles that don't belong to the document.
// Not `Copy`: the note author is a String, and the handful of places
// that snapshot the options clone explicitly.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ViewOptions {
    pub rulers: bool,
    pub grid: bool,
    pub guides: bool,
    /// Master switch for guides/grid/selection overlays (⌘H).
    pub extras: bool,
    pub snap: bool,
    pub grid_spacing: f32,
    /// Mobile follows the window's system appearance; old saved theme
    /// values are ignored there and omitted when preferences are saved.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    #[serde(default)]
    pub theme: Theme,
    /// Scroll zooms instead of panning (Photoshop's "Zoom with Scroll
    /// Wheel"). Useful on touchpads, where pinch gestures never arrive —
    /// GPUI doesn't surface them on any platform.
    #[serde(default)]
    pub zoom_with_scroll: bool,
    /// Write a local crash report when the editor panics. Opt-in, and
    /// nothing is ever transmitted.
    #[serde(default)]
    pub crash_reports: bool,
    /// Also upload that crash to the project's Sentry. Opt-in separately
    /// from the local report — writing a file here and sending one to us
    /// are not the same decision — and inert in any build that was not
    /// given a DSN, which is every build but the official releases.
    #[serde(default)]
    pub crash_upload: bool,
    /// Composite and resample on the GPU when an adapter exists. On by
    /// default; the CPU reference takes over per-frame for anything the
    /// GPU path can't express, and entirely when this is off.
    #[serde(default = "default_true")]
    pub gpu_compositing: bool,
    /// Ask GitHub for the latest release at launch, at most once a day.
    /// The one request Schist makes without being clicked, which is why
    /// it is a preference; it sends nothing but the request itself.
    #[serde(default = "default_true")]
    pub check_updates: bool,
    /// Draw note markers (View ▸ Notes, Photoshop's Show ▸ Notes).
    #[serde(default = "default_true")]
    pub notes: bool,
    /// The navigator/colour/layers/history column. Always shown on the
    /// desktop; the touch chrome has a button to fold it away, and on a
    /// phone-width window the toggle switches between it and the canvas.
    #[serde(default = "default_true")]
    pub side_panels: bool,
    /// Name stamped on notes as they are placed. A preference rather than
    /// document state: it is who is reviewing, not what is being
    /// reviewed, and typing it once per session would be once too many.
    #[serde(default = "default_note_author")]
    pub note_author: String,
    /// Colour new notes are given, 0xRRGGBB.
    #[serde(default = "default_note_color")]
    pub note_color: u32,
    /// Hide photos the gallery's content filter flags as explicit.
    /// Off by default, and honest about its needs: the judgement comes
    /// from the "Content (NSFW Filter)" model, fetched like any other
    /// under Filter ▸ Neural Filters ▸ Manage Models; without it,
    /// nothing is flagged.
    #[serde(default)]
    pub gallery_hide_nsfw: bool,
    /// Show the AI sidebar. Off by default: it spawns an agent CLI the
    /// user may not have, and a chat column is not everyone's furniture.
    #[serde(default)]
    pub ai_panel: bool,
    /// The same panel's switch for the gallery, remembered separately:
    /// the harness, model and conversation are shared between the two
    /// rooms, but whether a chat column sits beside the photos is its
    /// own question.
    #[serde(default)]
    pub ai_panel_gallery: bool,
    /// Which agent harness the sidebar drives ("claude" or "codex").
    #[serde(default = "default_ai_backend")]
    pub ai_backend: String,
    /// The model last used in this app, per harness — deliberately not
    /// the CLI's own default, which is tuned for coding. Empty until the
    /// first catalog fetch seeds it. Two fields because the slugs don't
    /// travel: "opus" means nothing to Codex, "gpt-5.5" nothing to
    /// Claude.
    #[serde(default)]
    pub ai_model_claude: String,
    #[serde(default)]
    pub ai_model_codex: String,
    /// The history panel's height. Fixed on the desktop; on touch the
    /// grip above its title drags it, and this remembers where.
    #[serde(default = "default_history_h")]
    pub history_h: f32,
    /// The camera-roll backup to Schist Cloud: whether it runs, from
    /// where, into which folder. Asked about once after the first
    /// sign-in on a phone. Desktop keeps the preference but does not
    /// run the backup; the browser uses separate preferences.
    #[cfg(not(target_arch = "wasm32"))]
    #[serde(default)]
    pub camera_sync: CameraSync,
}

fn default_history_h() -> f32 {
    if cfg!(any(target_os = "ios", target_os = "android")) {
        260.0
    } else {
        150.0
    }
}

/// Whoever is logged in, which is Photoshop's default author too. Empty
/// when the environment does not say, rather than a guess like "user".
fn default_note_author() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default()
}

fn default_note_color() -> u32 {
    let [r, g, b, _] = schist_core::DEFAULT_NOTE_COLOR.to_u8();
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn default_true() -> bool {
    true
}

fn default_ai_backend() -> String {
    "claude".to_string()
}

impl Default for ViewOptions {
    fn default() -> Self {
        ViewOptions {
            rulers: true,
            grid: false,
            guides: true,
            extras: true,
            snap: true,
            grid_spacing: 64.0,
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            theme: Theme::default(),
            zoom_with_scroll: false,
            crash_reports: false,
            crash_upload: false,
            gpu_compositing: true,
            check_updates: true,
            notes: true,
            side_panels: true,
            note_author: default_note_author(),
            note_color: default_note_color(),
            gallery_hide_nsfw: false,
            ai_panel: false,
            ai_panel_gallery: false,
            ai_backend: default_ai_backend(),
            ai_model_claude: String::new(),
            ai_model_codex: String::new(),
            history_h: default_history_h(),
            #[cfg(not(target_arch = "wasm32"))]
            camera_sync: Default::default(),
        }
    }
}

/// Persist view options so they survive a restart.
pub fn save_view_options(view: &ViewOptions) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(json) = serde_json::to_string(view) {
            schist_app_platform::web::local_set(schist_app_platform::web::PREFS_KEY, &json);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(path) = prefs_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(view) {
            let _ = std::fs::write(path, json);
        }
    }
}
