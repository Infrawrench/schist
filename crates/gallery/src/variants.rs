//! Named layered edits sharing one capture. The path is a stable identity;
//! names never participate in routing. No legacy sidecar is migrated.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize)]
struct Record {
    name: String,
    #[serde(default)]
    deleted: bool,
}

pub fn directory(original: &Path) -> Option<PathBuf> {
    Some(
        original
            .parent()?
            .join(".schist/variants")
            .join(original.file_name()?),
    )
}

/// Recognize only our reserved identity layout, never arbitrary PSDs.
pub fn original(path: &Path) -> Option<PathBuf> {
    let capture = crate::capture_original(path);
    (capture != path).then_some(capture)
}

pub fn capture(path: &Path) -> PathBuf {
    crate::capture_original(path)
}

fn read(path: &Path) -> io::Result<Record> {
    if original(path).is_none() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let record = path.with_extension("json");
    let metadata = fs::symlink_metadata(&record)?;
    if !metadata.is_file() || metadata.len() > 16384 {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let bytes = fs::read(record)?;
    let mut record: Record = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    record.name =
        checked_name(&record.name).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    Ok(record)
}

pub fn active(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) && read(path).is_ok_and(|r| !r.deleted)
}
pub fn name(path: &Path) -> Option<String> {
    read(path).ok().map(|r| r.name)
}
pub fn display_name(path: &Path) -> String {
    crate::photo_display_name(path)
}

/// A high-resolution cache revision, independent of the capture's date.
pub fn revision(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn unique() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:032x}{:08x}{:08x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn write(path: &Path, record: &Record) -> io::Result<()> {
    let target = path.with_extension("json");
    let tmp = path.with_extension(format!("{}.tmp", unique()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        file.write_all(&serde_json::to_vec(record).map_err(io::Error::other)?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}

fn checked_name(name: &str) -> io::Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok(name.to_owned())
}

/// Publish a fully encoded layered edit. The capture is never copied or written.
pub fn create(source: &Path, name: &str, psd: &[u8]) -> io::Result<PathBuf> {
    let name = checked_name(name)?;
    let capture = capture(source);
    #[cfg(not(target_arch = "wasm32"))]
    let _lock = crate::xmp::lock_sidecar(&capture).map_err(io::Error::other)?;
    if crate::is_video(&capture) || !capture.is_file() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    let dir = directory(&capture).ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.psd", unique()));
    let mut created = false;
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        created = true;
        file.write_all(psd)?;
        file.sync_all()?;
        drop(file);
        write(
            &path,
            &Record {
                name,
                deleted: false,
            },
        )?;
        // The name record publishes this identity only after all PSD bytes
        // are synced; interrupted PSD writes have no record and stay hidden.
        Ok(path.clone())
    })();
    if created && result.is_err() {
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("json"));
    }
    result
}

pub fn rename(path: &Path, name: &str) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    let _lock = crate::xmp::lock_sidecar(&capture(path)).map_err(io::Error::other)?;
    let mut record = read(path)?;
    if record.deleted {
        return Err(io::ErrorKind::NotFound.into());
    }
    record.name = checked_name(name)?;
    write(path, &record)
}

/// Hide the variant, retaining its last edit and full history for recovery.
pub fn delete(path: &Path) -> io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    let _lock = crate::xmp::lock_sidecar(&capture(path)).map_err(io::Error::other)?;
    let mut record = read(path)?;
    record.deleted = true;
    write(path, &record)
}

pub fn list(original: &Path) -> Vec<PathBuf> {
    stored(original)
        .into_iter()
        .filter(|path| active(path))
        .collect()
}

/// All stored copy identities, including deleted copies whose ratings/buckets
/// must follow a moved capture if the copy is later recovered.
pub fn stored(original: &Path) -> Vec<PathBuf> {
    let Some(dir) = directory(original) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| self::original(p).as_deref() == Some(original))
        .collect();
    paths.sort();
    paths
}

/// A portable export basename: user names cannot escape a chosen destination.
pub fn export_stem(path: &Path) -> String {
    if original(path).is_none() {
        return path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    }
    let capture = capture(path);
    let stem = capture.file_stem().unwrap_or_default().to_string_lossy();
    let name = display_name(path);
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{stem}-{safe}")
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Dir(PathBuf);
    impl Dir {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!("schist-variants-{}", unique()));
            fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn photo(&self, name: &str) -> PathBuf {
            let p = self.0.join(name);
            fs::write(&p, b"original capture").unwrap();
            p
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn same_second_edits_get_distinct_thumbnail_revisions() {
        let dir = Dir::new();
        let photo = dir.photo("capture.jpg");
        let variant = create(&photo, "Warm", b"layers").unwrap();
        let file = fs::OpenOptions::new().write(true).open(&variant).unwrap();
        file.set_times(
            fs::FileTimes::new()
                .set_modified(UNIX_EPOCH + std::time::Duration::new(42, 100_000_000)),
        )
        .unwrap();
        let first = revision(&variant);
        file.set_times(
            fs::FileTimes::new()
                .set_modified(UNIX_EPOCH + std::time::Duration::new(42, 900_000_000)),
        )
        .unwrap();
        let second = revision(&variant);
        assert_ne!(first, second);
        assert_ne!(
            crate::thumb_cache_path(&variant, first),
            crate::thumb_cache_path(&variant, second)
        );
    }

    #[test]
    fn malformed_or_unpublished_records_do_not_enter_the_gallery() {
        let dir = Dir::new();
        let photo = dir.photo("capture.jpg");
        let variant = create(&photo, "Warm", b"layers").unwrap();
        let record = variant.with_extension("json");
        fs::write(&record, br#"{"name":"bad\nname"}"#).unwrap();
        assert!(!active(&variant));
        assert!(list(&photo).is_empty());
        assert_eq!(
            crate::photo_display_name(&variant),
            variant.file_name().unwrap().to_string_lossy()
        );
        fs::write(&record, vec![b' '; 16385]).unwrap();
        assert!(!active(&variant));
        fs::remove_file(record).unwrap();
        assert!(list(&photo).is_empty());
        assert_eq!(fs::read(&photo).unwrap(), b"original capture");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn capture_lock_blocks_mutations_without_changing_records() {
        let dir = Dir::new();
        let photo = dir.photo("capture.jpg");
        let variant = create(&photo, "Warm", b"layers").unwrap();
        let lock = crate::xmp::lock_sidecar(&photo).unwrap();
        assert!(rename(&variant, "Cold").is_err());
        assert!(delete(&variant).is_err());
        assert!(create(&photo, "Copy", b"layers").is_err());
        assert_eq!(name(&variant).as_deref(), Some("Warm"));
        assert!(active(&variant));
        drop(lock);
        rename(&variant, "Cold").unwrap();
    }

    #[test]
    fn legacy_sidecars_are_unchanged_and_variants_have_independent_identity() {
        let dir = Dir::new();
        let photo = dir.photo("trip.jpg");
        let sidecar = crate::backing_psd(&photo).unwrap();
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        fs::write(&sidecar, b"primary edit").unwrap();
        crate::versions::keep(&sidecar).unwrap();
        assert!(list(&photo).is_empty());
        let a = create(&photo, "B&W", b"first layers").unwrap();
        let b = create(&photo, "Warm", b"second layers").unwrap();
        assert_ne!(a, b);
        assert_eq!(capture(&a), photo);
        assert_eq!(crate::backing_psd(&a), Some(a.clone()));
        assert_ne!(
            crate::thumb_cache_path(&a, 1),
            crate::thumb_cache_path(&b, 1)
        );
        assert_eq!(fs::read(&photo).unwrap(), b"original capture");
        assert_eq!(fs::read(&sidecar).unwrap(), b"primary edit");
        assert_eq!(crate::versions::list(&photo).unwrap().len(), 3);
        assert_eq!(list(&photo).len(), 2);
        let scan = crate::scan_folders(std::slice::from_ref(&dir.0), &["jpg".into(), "psd".into()]);
        assert_eq!(scan.len(), 1);
        assert_eq!(scan[0].entries.len(), 3);
        assert_eq!(scan[0].entries[0].path, photo);
    }

    #[test]
    fn rename_delete_and_histories_preserve_original_and_siblings() {
        let dir = Dir::new();
        let photo = dir.photo("a.raw");
        let a = create(&photo, "Warm", b"first").unwrap();
        let b = create(&a, "Square", b"second").unwrap();
        crate::versions::keep(&a).unwrap();
        fs::write(&a, b"edited first").unwrap();
        rename(&a, "Monochrome").unwrap();
        assert_eq!(name(&a).as_deref(), Some("Monochrome"));
        let history = crate::versions::list(&a).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history.last().unwrap().path, photo);
        assert_eq!(crate::versions::list(&b).unwrap().len(), 2);
        delete(&a).unwrap();
        assert!(!active(&a));
        assert_eq!(list(&photo), vec![b.clone()]);
        assert_eq!(fs::read(&a).unwrap(), b"edited first");
        assert_eq!(fs::read(&history[1].path).unwrap(), b"first");
        assert_eq!(fs::read(&b).unwrap(), b"second");
        assert_eq!(fs::read(&photo).unwrap(), b"original capture");
        assert!(rename(&a, "resurrect").is_err());
    }

    #[test]
    fn names_are_not_paths_and_unrelated_files_are_not_variants() {
        let dir = Dir::new();
        let a = dir.photo("a.jpg");
        let b = dir.photo("a.png");
        assert!(create(&a, " \n ", b"layers").is_err());
        let variant = create(&a, "../warm/crop", b"layers").unwrap();
        assert!(!export_stem(&variant).contains('/'));
        assert!(list(&b).is_empty());
        assert!(original(&a).is_none());
        assert!(original(&dir.0.join("other.psd")).is_none());
        assert!(create(&dir.photo("a.mp4"), "video", b"layers").is_err());
        assert!(delete(&a).is_err());
    }
}
