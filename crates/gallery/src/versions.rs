//! The saved PSD versions belonging to a single gallery original.
//!
//! Legacy names are `<seconds>-<sidecar>`; additional saves in that second
//! use `<seconds>.<sequence>-<sidecar>`. Matching the complete suffix and
//! parsing the entire prefix keeps similarly named neighbours separate.

use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionKind {
    Current,
    Saved { seconds: u64, sequence: u64 },
    Original,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub path: PathBuf,
    pub kind: VersionKind,
}

/// Current edit, saved edits newest first, and the original. Missing
/// sidecars/archives are normal; unreadable archives are reported.
pub fn list(original: &Path) -> io::Result<Vec<Version>> {
    let Some(sidecar) = crate::backing_psd(original) else {
        return Ok(Vec::new());
    };
    let mut versions = Vec::new();
    if sidecar.is_file() {
        versions.push(Version {
            path: sidecar.clone(),
            kind: VersionKind::Current,
        });
    }
    let suffix = format!("-{}", sidecar.file_name().unwrap().to_string_lossy());
    let directory = sidecar.parent().unwrap().join("versions");
    let mut saved = Vec::new();
    match fs::read_dir(directory) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let name = entry.file_name();
                let Some((seconds, sequence)) = name.to_str().and_then(|n| parse_name(n, &suffix))
                else {
                    continue;
                };
                saved.push(Version {
                    path: entry.path(),
                    kind: VersionKind::Saved { seconds, sequence },
                });
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    saved.sort_by_key(|entry| match entry.kind {
        VersionKind::Saved { seconds, sequence } => std::cmp::Reverse((seconds, sequence)),
        _ => unreachable!(),
    });
    versions.extend(saved);
    versions.push(Version {
        path: original.to_path_buf(),
        kind: VersionKind::Original,
    });
    Ok(versions)
}

fn parse_name(name: &str, suffix: &str) -> Option<(u64, u64)> {
    let stamp = name.strip_suffix(suffix)?;
    let (seconds, sequence) = stamp.split_once('.').unwrap_or((stamp, "0"));
    if seconds.is_empty()
        || sequence.is_empty()
        || !seconds.bytes().all(|c| c.is_ascii_digit())
        || !sequence.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some((seconds.parse().ok()?, sequence.parse().ok()?))
}

/// Preserve a sidecar without overwriting another snapshot, even when
/// saves happen in the same second. Creates the sidecar's parent for its
/// first save. Incomplete copies are removed and errors reach the caller.
pub fn keep(sidecar: &Path) -> io::Result<Option<PathBuf>> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    keep_at(sidecar, seconds)
}

fn keep_at(sidecar: &Path, seconds: u64) -> io::Result<Option<PathBuf>> {
    let parent = sidecar
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    fs::create_dir_all(parent)?;
    let mut source = match fs::File::open(sidecar) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let directory = parent.join("versions");
    fs::create_dir_all(&directory)?;
    let name = sidecar
        .file_name()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?
        .to_string_lossy();
    for sequence in 0u64.. {
        let stamp = if sequence == 0 {
            seconds.to_string()
        } else {
            format!("{seconds}.{sequence}")
        };
        let path = directory.join(format!("{stamp}-{name}"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut output) => {
                if let Err(error) =
                    io::copy(&mut source, &mut output).and_then(|_| output.sync_all())
                {
                    drop(output);
                    let _ = fs::remove_file(&path);
                    return Err(error);
                }
                return Ok(Some(path));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("version sequence exhausted"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "schist-versions-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn versions_match_exact_photo_and_sort_numerically() {
        let dir = Directory::new();
        let original = dir.0.join("a.jpg");
        let sidecar = crate::backing_psd(&original).unwrap();
        let archive = sidecar.parent().unwrap().join("versions");
        fs::create_dir_all(&archive).unwrap();
        fs::write(&original, b"original").unwrap();
        fs::write(&sidecar, b"current").unwrap();
        for name in [
            "9-a.jpg.psd",
            "10-a.jpg.psd",
            "10.2-a.jpg.psd",
            "10.11-a.jpg.psd",
            "99-ba.jpg.psd",
            "99-other-a.jpg.psd",
            "garbage-a.jpg.psd",
            "99-a.png.psd",
        ] {
            fs::write(archive.join(name), b"snapshot").unwrap();
        }
        fs::create_dir(archive.join("99-a.jpg.psd")).unwrap();
        let versions = list(&original).unwrap();
        assert_eq!(versions.len(), 6);
        assert_eq!(versions[0].kind, VersionKind::Current);
        assert_eq!(
            versions[1].kind,
            VersionKind::Saved {
                seconds: 10,
                sequence: 11
            }
        );
        assert_eq!(
            versions[2].kind,
            VersionKind::Saved {
                seconds: 10,
                sequence: 2
            }
        );
        assert_eq!(
            versions[3].kind,
            VersionKind::Saved {
                seconds: 10,
                sequence: 0
            }
        );
        assert_eq!(
            versions[4].kind,
            VersionKind::Saved {
                seconds: 9,
                sequence: 0
            }
        );
        assert_eq!(versions[5].kind, VersionKind::Original);
        fs::remove_file(sidecar).unwrap();
        assert_eq!(
            list(&original).unwrap().len(),
            5,
            "reverted edits retain their history"
        );
    }

    #[test]
    fn rapid_saves_keep_distinct_bytes_and_original_is_untouched() {
        let dir = Directory::new();
        let original = dir.0.join("a.jpg");
        fs::write(&original, b"original").unwrap();
        let sidecar = crate::backing_psd(&original).unwrap();
        assert!(keep_at(&sidecar, 42).unwrap().is_none());
        fs::write(&sidecar, b"first").unwrap();
        let first = keep_at(&sidecar, 42).unwrap().unwrap();
        fs::write(&sidecar, b"second").unwrap();
        let second = keep_at(&sidecar, 42).unwrap().unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(first).unwrap(), b"first");
        assert_eq!(fs::read(second).unwrap(), b"second");
        assert_eq!(fs::read(sidecar).unwrap(), b"second");
        assert_eq!(fs::read(original).unwrap(), b"original");
    }

    #[test]
    fn unedited_photos_have_original_and_videos_have_no_versions() {
        let dir = Directory::new();
        assert_eq!(
            list(&dir.0.join("a.jpg")).unwrap()[0].kind,
            VersionKind::Original
        );
        assert!(list(&dir.0.join("a.mp4")).unwrap().is_empty());
    }

    #[test]
    fn concurrent_snapshots_keep_hyphenated_photos_separate() {
        let dir = Directory::new();
        let original = dir.0.join("summer-trip.final.jpg");
        let sidecar = crate::backing_psd(&original).unwrap();
        keep_at(&sidecar, 42).unwrap();
        fs::write(&sidecar, b"layered edit").unwrap();
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let sidecar = sidecar.clone();
                std::thread::spawn(move || keep_at(&sidecar, 42).unwrap().unwrap())
            })
            .collect();
        let paths: std::collections::HashSet<_> =
            workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(paths.len(), 8);
        for path in &paths {
            assert_eq!(fs::read(path).unwrap(), b"layered edit");
        }
        assert_eq!(list(&original).unwrap().len(), 10);
        assert_eq!(list(&dir.0.join("trip.final.jpg")).unwrap().len(), 1);
    }
}
