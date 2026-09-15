#[cfg(any(target_os = "ios", target_os = "android"))]
use schist_i18n::t;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The rule, as preferences.json keeps it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CameraSync {
    /// Whether the backup runs. Off until the prompt is accepted.
    #[serde(default)]
    pub enabled: bool,
    /// Separates checkpoints when the destination or signed-in account changes.
    #[serde(default)]
    pub ledger_key: String,
    /// Whether the sign-in prompt has been shown, accepted or not: it
    /// asks once. Preferences reopens the same dialog.
    #[serde(default)]
    pub asked: bool,
    #[serde(default)]
    pub source: Option<Source>,
    /// The cloud folder photos go into; `None` files them unfiled.
    #[serde(default)]
    pub folder_id: Option<String>,
    /// Its name when the rule was made, for the sidebar's caption — the
    /// folder may since have been renamed, which is only cosmetic here.
    #[serde(default)]
    pub folder_name: String,
    /// When the last successful run finished, seconds since the epoch.
    #[serde(default)]
    pub last_run: Option<u64>,
    /// What the last run said when it did not finish.
    #[serde(default)]
    pub last_error: Option<String>,
    /// The extensions the app's codecs decode, lower-case, as they stood
    /// when the rule was made: what counts as a photo. Kept with the rule
    /// so a headless run (Android's job, which has no codec registry)
    /// agrees with the app.
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// Where photos come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// A folder on disk, walked like a drag into the cloud gallery:
    /// hidden entries and symlinks stay home, sub-folders become cloud
    /// folders.
    Folder { path: PathBuf },
    /// A Photos album on iOS by its local identifier; `library`
    /// is the whole library.
    Album { id: String, title: String },
}
/// The album id that means every photo in the library rather than one
/// album. It survives a change of authorization, which a smart album's
/// real identifier, fetched under the old one, need not.
#[cfg(any(target_os = "ios", target_os = "android"))]
pub const LIBRARY_ALBUM: &str = "library";
#[cfg(any(target_os = "ios", target_os = "android"))]
impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Folder { path } => schist_app_platform::shown_path(path),
            Source::Album { id, title } if id == LIBRARY_ALBUM || title.is_empty() => {
                t("cloud.sync.all_photos").to_string()
            }
            Source::Album { title, .. } => title.clone(),
        }
    }
}
