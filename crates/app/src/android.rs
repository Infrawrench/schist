//! The process environment on Android.
//!
//! An Android app's process starts with none of the variables Schist's
//! paths are derived from: no `HOME`, and a `TMPDIR` (`/data/local/tmp`)
//! the app cannot write. Everything that looks for a home -- the
//! preferences and library under `.config/schist`, the caches, recovery
//! files and index under `.local/state/schist`, a downloaded libheif
//! under `.local/share/schist` -- is pointed at the app's own storage
//! here, before anything reads them, the way iOS gives the app a `HOME`
//! of its container.
//!
//! `HOME` is the app's internal files directory (`/data/user/0/<package>/
//! files`): private, and on the one filesystem a library can be
//! `dlopen`ed from (external storage is mounted `noexec`); `TMPDIR` is
//! the cache directory beside it. Documents go elsewhere: [`documents_dir`]
//! is `Documents` under the app's external files directory
//! (`/sdcard/Android/data/<package>/files`), which is still the app's own
//! but reachable over USB and `adb`, so what the user saves can be got at.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The app's document folder, once [`prepare_environment`] has found it.
static DOCUMENTS: OnceLock<PathBuf> = OnceLock::new();

/// Point `HOME` and `TMPDIR` at the app's storage and find the documents
/// folder. Called first thing in `main`, before any thread is spawned,
/// which is when setting an environment variable is sound.
pub fn prepare_environment() {
    let Some(app) = gpui::android::app() else {
        // Not started by an activity (a test binary run from a shell):
        // the shell's environment stands.
        return;
    };
    let Some(home) = app.internal_data_path() else {
        log::error!("the app has no storage directory; paths will not resolve");
        return;
    };
    std::env::set_var("HOME", &home);
    std::env::set_var("TMPDIR", &cache_dir(&home));
    // External storage when there is any (every phone and emulator), the
    // private directory otherwise.
    let documents = app
        .external_data_path()
        .filter(|dir| std::fs::create_dir_all(dir).is_ok())
        .unwrap_or_else(|| home.clone())
        .join("Documents");
    if let Err(err) = std::fs::create_dir_all(&documents) {
        log::warn!("could not create {}: {err}", documents.display());
    }
    DOCUMENTS.set(documents).ok();
}

/// Where documents are saved and imports land; `None` before
/// [`prepare_environment`] has run, or when the app has no storage.
pub fn documents_dir() -> Option<PathBuf> {
    DOCUMENTS.get().cloned()
}

/// The cache directory that goes with a files directory: Android lays
/// out `<package>/files` and `<package>/cache` side by side. Falls back
/// to a `cache` inside the files directory when that layout is not
/// there, so the temporary files always have somewhere writable.
fn cache_dir(home: &Path) -> PathBuf {
    let sibling = home.parent().map(|parent| parent.join("cache"));
    match sibling {
        Some(dir) if std::fs::create_dir_all(&dir).is_ok() => dir,
        _ => {
            let dir = home.join("cache");
            std::fs::create_dir_all(&dir).ok();
            dir
        }
    }
}
