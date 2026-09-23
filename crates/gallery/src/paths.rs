//! Where the gallery keeps things.

use std::hash::{Hash as _, Hasher as _};
use std::path::{Path, PathBuf};

/// Longest edge of a rendered thumbnail. Cells scale the image down from
/// here, so one render serves every position of the size slider — and
/// it is part of the disk-cache key, so a change re-renders the lot.
pub const THUMB_EDGE: u32 = 256;

fn home_storage_dir(unix_subdir: &str) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").ok()?);
    let legacy = home.join(unix_subdir);
    // A device sandbox permits app data under Library, not arbitrary dot
    // directories at its root. Keep existing simulator data and recoveries
    // in place; older simulator builds could write the Unix locations.
    #[cfg(target_os = "ios")]
    if !legacy.join("schist").exists() {
        return Some(home.join("Library/Application Support"));
    }
    Some(legacy)
}

/// The per-user state directory (`~/.local/state` on Unix, LOCALAPPDATA
/// on Windows, Library/Application Support on iOS): caches, recoveries,
/// and the update stamp.
pub fn state_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var("LOCALAPPDATA")
            .or_else(|_| std::env::var("USERPROFILE"))
            .ok()
            .map(PathBuf::from)
    } else {
        std::env::var("XDG_STATE_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| home_storage_dir(".local/state"))
    }
}

/// `library.json`: the watched folders, buckets and recents.
pub fn library_path() -> Option<PathBuf> {
    let base = std::env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| home_storage_dir(".config"))?;
    Some(base.join("schist/library.json"))
}

/// Where the index snapshot lives between runs.
pub fn index_snapshot_path() -> Option<PathBuf> {
    Some(state_dir()?.join("schist/index.v1"))
}

/// Where the People sidebar's last counts live between runs, so the
/// numbers are right from the first frame rather than creeping up as
/// the index is read back.
pub fn people_summary_path() -> Option<PathBuf> {
    Some(state_dir()?.join("schist/people.json"))
}

/// The PSD sidecar an edit of `original` saves into.
pub fn backing_psd(original: &Path) -> Option<PathBuf> {
    if crate::is_video(original) {
        return None;
    }
    let dir = original.parent()?;
    let name = original.file_name()?.to_string_lossy();
    Some(dir.join(".schist").join(format!("{name}.psd")))
}

/// What a thumbnail renders from: the sidecar once one exists, so the
/// gallery shows the edit, as Picasa does.
pub fn thumb_source(original: &Path, edited: bool) -> PathBuf {
    if edited {
        if let Some(psd) = backing_psd(original) {
            return psd;
        }
    }
    original.to_path_buf()
}

/// Modification time in seconds since the epoch; zero when unreadable.
pub fn mtime_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Where rendered thumbnails are cached between runs, keyed by source
/// path, mtime and render size — a re-edited photo gets a fresh entry
/// and the stale one ages out with the directory. The score, embedding
/// and metadata caches sit beside it under the same stem.
pub fn thumb_cache_path(source: &Path, mtime: u64) -> Option<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    mtime.hash(&mut hasher);
    THUMB_EDGE.hash(&mut hasher);
    let dir = state_dir()?.join("schist/thumbs");
    Some(dir.join(format!("{:016x}.png", hasher.finish())))
}

/// Resolve a gallery virtual-copy identity to the shared original capture.
/// Ordinary file paths pass through unchanged. This is purely lexical.
pub fn capture_original(path: &Path) -> PathBuf {
    fn variant_original(path: &Path) -> Option<PathBuf> {
        if path.extension()? != "psd" {
            return None;
        }
        let id = path.file_stem()?.to_str()?;
        if id.len() != 48 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let photo = path.parent()?;
        let variants = photo.parent()?;
        let hidden = variants.parent()?;
        if variants.file_name()? != "variants" || hidden.file_name()? != ".schist" {
            return None;
        }
        Some(hidden.parent()?.join(photo.file_name()?))
    }
    variant_original(path).unwrap_or_else(|| path.to_owned())
}

/// Human-readable gallery name; virtual-copy names do not change file identity.
pub fn photo_display_name(path: &Path) -> String {
    let variant_name = || -> Option<String> {
        if capture_original(path) == path {
            return None;
        }
        let record = path.with_extension("json");
        let metadata = std::fs::symlink_metadata(&record).ok()?;
        if !metadata.is_file() || metadata.len() > 16384 {
            return None;
        }
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(record).ok()?).ok()?;
        let name = value.get("name")?.as_str()?.trim();
        if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
            return None;
        }
        Some(name.to_owned())
    };
    variant_name().unwrap_or_else(|| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sidecar_lives_in_a_hidden_directory_beside_the_photo() {
        // The sidecar carries the extension of the original in its name,
        // so `a.jpg` and `a.png` in one folder never share an edit.
        assert_eq!(
            backing_psd(Path::new("/photos/trip/a.jpg")),
            Some(PathBuf::from("/photos/trip/.schist/a.jpg.psd"))
        );
        assert_eq!(
            backing_psd(Path::new("/photos/trip/a.png")),
            Some(PathBuf::from("/photos/trip/.schist/a.png.psd"))
        );
    }

    #[test]
    fn thumb_cache_keys_change_with_the_file() {
        let a = thumb_cache_path(Path::new("/p/a.jpg"), 1);
        let b = thumb_cache_path(Path::new("/p/a.jpg"), 2);
        let c = thumb_cache_path(Path::new("/p/b.jpg"), 1);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, thumb_cache_path(Path::new("/p/a.jpg"), 1));
    }
    #[test]
    fn printing_virtual_caption_name_and_capture_are_independent_of_edit_identity() {
        let dir = tempfile::tempdir().unwrap();
        let capture = dir.path().join("photo.jpg");
        let variant_dir = dir.path().join(".schist/variants/photo.jpg");
        std::fs::create_dir_all(&variant_dir).unwrap();
        let variant = variant_dir.join(format!("{}.psd", "0".repeat(48)));
        std::fs::write(
            variant.with_extension("json"),
            br#"{"name":"Warm print","deleted":false}"#,
        )
        .unwrap();
        assert_eq!(capture_original(&variant), capture);
        assert_eq!(photo_display_name(&variant), "Warm print");
        assert_eq!(photo_display_name(&capture), "photo.jpg");
        std::fs::write(variant.with_extension("json"), br#"{"name":"bad\nname"}"#).unwrap();
        assert_eq!(
            photo_display_name(&variant),
            variant.file_name().unwrap().to_string_lossy()
        );
        let unrelated = variant_dir.join("ordinary.psd");
        assert_eq!(capture_original(&unrelated), unrelated);
    }
}
