//! Tethered capture session settings and collision-safe publication.
use schist_i18n::t;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub type Result<T> = std::result::Result<T, String>;
pub mod ptp;

#[cfg(target_os = "macos")]
pub mod webcam;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Camera {
    pub id: Option<u64>,
    pub model: String,
    pub port: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub destination: PathBuf,
    pub prefix: String,
}
impl Session {
    pub fn validate(&self) -> Result<()> {
        if !self.destination.is_absolute() || !self.destination.is_dir() {
            return Err(t("common.file_not_found").into());
        }
        if self.prefix.is_empty()
            || self.prefix.len() > 120
            || self.prefix.starts_with('.')
            || self
                .prefix
                .chars()
                .any(|c| c.is_control() || "/\\:%<>\"|?*".contains(c))
        {
            return Err(t("tethered.invalid_name").into());
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Option<Self>> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(io_error),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error(e)),
        }
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let parent = path
            .parent()
            .ok_or_else(|| t("common.file_not_found").to_string())?;
        fs::create_dir_all(parent).map_err(io_error)?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(io_error)?;
        serde_json::to_writer(&mut file, self).map_err(io_error)?;
        file.as_file().sync_all().map_err(io_error)?;
        file.persist(path).map_err(io_error)?;
        Ok(())
    }
}

fn io_error(error: impl std::fmt::Display) -> String {
    schist_i18n::tf!("library.ops.save_failed", error = error)
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(t("common.cancelled").into())
    } else {
        Ok(())
    }
}

pub fn staging(session: &Session) -> Result<tempfile::TempDir> {
    session.validate()?;
    tempfile::Builder::new()
        .prefix(".schist-capture-")
        .tempdir_in(&session.destination)
        .map_err(io_error)
}

#[derive(Debug)]
pub struct Captured {
    pub paths: Vec<PathBuf>,
    pub warning: Option<String>,
}

/// Never use a camera-supplied path as a local filename. Only a validated
/// extension crosses the device/filesystem boundary; indices avoid collisions
/// when different camera folders contain identically named originals.
pub fn download_path(staging: &Path, index: usize, name: &str) -> Result<PathBuf> {
    let extension = name.rsplit_once('.').map(|(_, extension)| extension);
    if name.chars().any(|c| c.is_control() || "/\\:".contains(c))
        || extension.is_none_or(|ext| {
            ext.is_empty() || ext.len() > 12 || !ext.bytes().all(|c| c.is_ascii_alphanumeric())
        })
    {
        return Err(t("tethered.no_download").into());
    }
    Ok(staging.join(format!(
        "{index:06}.{}",
        extension.unwrap().to_ascii_lowercase()
    )))
}

pub fn publish(staging: &Path, session: &Session, cancel: &AtomicBool) -> Result<Captured> {
    publish_with(staging, session, cancel, |source, dest| {
        source.persist_noclobber(dest)
    })
}
fn publish_with(
    staging: &Path,
    session: &Session,
    cancel: &AtomicBool,
    mut publish: impl FnMut(
        tempfile::TempPath,
        &Path,
    ) -> std::result::Result<(), tempfile::PathPersistError>,
) -> Result<Captured> {
    session.validate()?;
    cancelled(cancel)?;
    let mut sources = fs::read_dir(staging)
        .map_err(io_error)?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(io_error)?;
    sources.sort();
    if sources.is_empty() {
        return Err(t("tethered.no_download").into());
    }
    for path in &sources {
        let meta = fs::symlink_metadata(path).map_err(io_error)?;
        if !meta.is_file()
            || meta.len() == 0
            || path.extension().and_then(|e| e.to_str()).is_none_or(|ext| {
                ext.is_empty() || ext.len() > 12 || !ext.bytes().all(|c| c.is_ascii_alphanumeric())
            })
        {
            return Err(t("tethered.no_download").into());
        }
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .and_then(|file| file.sync_all())
            .map_err(io_error)?;
    }
    cancelled(cancel)?;
    let mut seen = std::collections::HashMap::<String, usize>::new();
    let suffixes: Vec<_> = sources
        .iter()
        .map(|source| {
            let ext = source
                .extension()
                .unwrap()
                .to_string_lossy()
                .to_ascii_lowercase();
            let count = seen.entry(ext.clone()).or_default();
            *count += 1;
            if *count == 1 {
                format!(".{ext}")
            } else {
                format!("-{}.{ext}", *count)
            }
        })
        .collect();
    let mut pending: std::collections::VecDeque<_> = sources
        .into_iter()
        .map(tempfile::TempPath::try_from_path)
        .collect::<std::io::Result<_>>()
        .map_err(io_error)?;
    let mut sequence = 1u64;
    loop {
        cancelled(cancel)?;
        let destinations: Vec<_> = suffixes
            .iter()
            .map(|suffix| {
                session
                    .destination
                    .join(format!("{}-{sequence:06}{suffix}", session.prefix))
            })
            .collect();
        let mut occupied = false;
        for dest in &destinations {
            match fs::symlink_metadata(dest) {
                Ok(_) => {
                    occupied = true;
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_error(e)),
            }
        }
        if occupied {
            sequence += 1;
            continue;
        }
        let mut published = Vec::new();
        for dest in destinations {
            let source = pending.pop_front().unwrap();
            match publish(source, &dest) {
                Ok(()) => published.push(dest),
                Err(error) => {
                    let collision = error.error.kind() == std::io::ErrorKind::AlreadyExists;
                    pending.push_front(error.path);
                    if published.is_empty() && collision {
                        break;
                    }
                    let error = io_error(error.error);
                    if published.is_empty() {
                        return Err(error);
                    }
                    return Ok(Captured {
                        paths: published,
                        warning: Some(error),
                    });
                }
            }
        }
        if !published.is_empty() {
            return Ok(Captured {
                paths: published,
                warning: None,
            });
        }
        sequence += 1;
    }
}

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::{capture, connect, discover};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{capture, connect, discover};

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::{begin_android, cancel_android, capture, connect, discover};

#[cfg(test)]
mod tests {
    use super::*;

    fn session(dir: &Path) -> Session {
        Session {
            destination: dir.to_path_buf(),
            prefix: "shoot ' $()".into(),
        }
    }

    #[test]
    fn originals_publish_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let session = session(dir.path());
        let staging = staging(&session).unwrap();
        fs::write(staging.path().join("capture.JPG"), b"jpeg").unwrap();
        fs::write(staging.path().join("capture.CR3"), b"raw").unwrap();
        let existing = dir.path().join(format!("{}-000001.jpg", session.prefix));
        fs::write(&existing, b"original").unwrap();
        let result = publish(staging.path(), &session, &AtomicBool::new(false)).unwrap();
        assert!(result.warning.is_none());
        assert_eq!(result.paths.len(), 2);
        assert_eq!(fs::read(existing).unwrap(), b"original");
        assert!(result
            .paths
            .contains(&dir.path().join(format!("{}-000002.jpg", session.prefix))));
        assert!(result
            .paths
            .contains(&dir.path().join(format!("{}-000002.cr3", session.prefix))));
        drop(staging);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn late_collision_retries_whole_set_or_reports_committed_files() {
        for collision_at in [0, 1] {
            let dir = tempfile::tempdir().unwrap();
            let session = session(dir.path());
            let staging = staging(&session).unwrap();
            fs::write(staging.path().join("capture-1.jpg"), b"jpeg").unwrap();
            fs::write(staging.path().join("capture-2.cr3"), b"raw").unwrap();
            let mut calls = 0;
            let mut foreign = None;
            let result = publish_with(
                staging.path(),
                &session,
                &AtomicBool::new(false),
                |source, dest| {
                    if calls == collision_at {
                        fs::write(dest, b"another process").unwrap();
                        foreign = Some(dest.to_path_buf());
                    }
                    calls += 1;
                    source.persist_noclobber(dest)
                },
            )
            .unwrap();
            assert_eq!(fs::read(foreign.unwrap()).unwrap(), b"another process");
            if collision_at == 0 {
                assert!(result.warning.is_none());
                assert_eq!(result.paths.len(), 2);
                assert!(result.paths.iter().all(|path| path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains("000002")));
            } else {
                assert!(result.warning.is_some());
                assert_eq!(result.paths.len(), 1);
                assert_eq!(fs::read(&result.paths[0]).unwrap(), b"jpeg");
            }
        }
    }

    #[test]
    fn cancellation_and_incomplete_downloads_never_import() {
        let dir = tempfile::tempdir().unwrap();
        let session = session(dir.path());
        for (name, bytes) in [
            ("capture", b"partial".as_slice()),
            ("capture.jpg", b"".as_slice()),
        ] {
            let staging = staging(&session).unwrap();
            fs::write(staging.path().join(name), bytes).unwrap();
            assert!(publish(staging.path(), &session, &AtomicBool::new(false)).is_err());
        }
        let staging_dir = staging(&session).unwrap();
        fs::write(staging_dir.path().join("capture.jpg"), b"photo").unwrap();
        assert!(publish(staging_dir.path(), &session, &AtomicBool::new(true)).is_err());
        let empty_staging = staging(&session).unwrap();
        assert!(publish(empty_staging.path(), &session, &AtomicBool::new(false)).is_err());
        drop(staging_dir);
        drop(empty_staging);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn configuration_validates_and_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut session = session(dir.path());
        session.save(&path).unwrap();
        let restored = Session::load(&path).unwrap().unwrap();
        assert_eq!(restored.destination, session.destination);
        assert_eq!(restored.prefix, session.prefix);
        for bad in [
            "",
            "../escape",
            ".hidden",
            "a/b",
            "a\\b",
            "a%f",
            "a:b",
            "a\nb",
            "a?b",
            "a*b",
            "a<b",
            "a>b",
            "a|b",
            "a\"b",
        ] {
            session.prefix = bad.into();
            assert!(session.validate().is_err(), "{bad:?}");
        }
        fs::write(path, "{broken").unwrap();
        assert!(Session::load(&dir.path().join("settings.json")).is_err());
    }

    #[test]
    fn camera_names_cannot_escape_staging_or_collide() {
        let root = Path::new("staging");
        for name in [
            "../photo.jpg",
            "/photo.jpg",
            "a\\photo.jpg",
            "C:photo.jpg",
            "photo",
            "a.j\ng",
            "a.",
            "a.abcdefghijklmn",
        ] {
            assert!(download_path(root, 0, name).is_err(), "{name}");
        }
        assert_eq!(
            download_path(root, 0, "photo.JPG").unwrap(),
            root.join("000000.jpg")
        );
        assert_ne!(
            download_path(root, 0, "photo.JPG").unwrap(),
            download_path(root, 1, "photo.JPG").unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_download_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let session = session(dir.path());
        let staging = staging(&session).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", staging.path().join("capture.jpg")).unwrap();
        assert!(publish(staging.path(), &session, &AtomicBool::new(false)).is_err());
        drop(staging);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
