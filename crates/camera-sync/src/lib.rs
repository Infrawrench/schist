//! Backing up the camera roll to Schist Cloud.
//!
//! A rule — one source of photos, one cloud folder — kept with the
//! preferences, and an engine that brings the folder up to date with the
//! source: every photo the ledger has not seen goes through the same
//! upload pipeline a drag into the cloud gallery uses (`cloud.rs`'s
//! `upload_files`), which hashes it, asks the provider whether the
//! library already holds it, and sends the rest up in compressed batches.
//! The ledger remembers what has been handled so a roll of twenty
//! thousand photos costs one walk per run rather than twenty thousand
//! hashes.
//!
//! The engine knows nothing of the workspace: on Android a `JobService`
//! runs it headless, with no activity and no gpui, to catch up while the
//! app is closed (`camera_sync_android.rs`). The workspace half here
//! drives it while the app is open — after a sign-in, every quarter hour,
//! when the source changes — and asks, once, after the first sign-in on a
//! phone, whether to set it up at all.
//!
//! Sources differ by platform. Android's camera roll is a folder
//! (`DCIM/Camera` under the shared storage) once the media permission is
//! granted, so the source is a path. iOS's is not: photos come from the
//! Photos framework by identifier, and each one is written to a staging
//! folder for the upload and removed after (`camera_sync_ios.rs`).

use schist_cloud_transfer::{upload_files, Handled, Report};

use anyhow::{anyhow, Result};
use schist_cloud as remote;
use schist_i18n::{t, tf};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime},
};

#[cfg(any(target_os = "ios", target_os = "android"))]
pub use schist_app_settings::LIBRARY_ALBUM;
pub use schist_app_settings::{CameraSync, Source};

/// What has been handled: the identity of every photo that went up or
/// was found already in the library. Kept as JSON beside the cloud's
/// other state; a missing or unreadable ledger just means one full
/// pass, which the provider's own duplicate check makes harmless.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    pub seen: HashSet<String>,
    #[serde(default)]
    pub rule: String,
}
impl Ledger {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    /// Written whole to a sibling and moved into place, so a run cut
    /// short by the OS leaves the previous ledger rather than half of
    /// this one.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}
/// The ledger's place: with the cloud's recovery checkpoints.
pub fn ledger_path() -> PathBuf {
    state_dir().join("camera-sync.json")
}
/// Where an iOS photo is written for its upload.
pub fn staging_dir() -> PathBuf {
    std::env::temp_dir().join("schist-camera-sync")
}

/// One photo the source offers.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// What the ledger records: the path with its mtime and size, or the
    /// asset's identifier with its modification date — either changes
    /// when the photo does.
    pub identity: String,
    pub name: String,
    /// The path inside the source, so its sub-folders become cloud
    /// folders; `None` for a photo straight from the library.
    pub relative: Option<String>,
    pub locate: Locate,
}
#[derive(Debug, Clone)]
pub enum Locate {
    Path(PathBuf),
    /// A Photos asset, to be written out before upload.
    #[cfg_attr(not(target_os = "ios"), allow(dead_code))]
    Asset(String),
}

/// How deep a source folder is walked. The camera roll is flat, or one
/// level of albums; this only stops a runaway.
const MAX_DEPTH: usize = 8;
/// Photos per pass through the pipeline: bounds the staging folder on
/// iOS and how much is lost to an interruption on either platform, since
/// the ledger is written after each chunk.
const CHUNK: usize = 100;

/// The photos under `root` whose extension is one Schist decodes, in a
/// stable order. Same rules as a drag into the cloud: hidden entries and
/// symlinks are left out.
pub fn list_folder(root: &Path, extensions: &HashSet<String>) -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    walk(root, root, 0, extensions, &mut out)?;
    out.sort_by(|a, b| a.identity.cmp(&b.identity));
    Ok(out)
}
fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    extensions: &HashSet<String>,
    out: &mut Vec<Candidate>,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if ty.is_dir() {
            // A sub-folder that cannot be read (another app's private
            // media) is skipped, not fatal.
            if let Err(error) = walk(root, &path, depth + 1, extensions, out) {
                log::warn!("camera sync: skipping {}: {error}", path.display());
            }
            continue;
        }
        if !ty.is_file() {
            continue;
        }
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !extensions.contains(&extension) {
            continue;
        }
        let metadata = entry.metadata()?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let relative = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .ok();
        out.push(Candidate {
            identity: format!("{}|{mtime}|{}", path.display(), metadata.len()),
            name: entry.file_name().to_string_lossy().into_owned(),
            relative,
            locate: Locate::Path(path),
        });
    }
    Ok(())
}

/// One run's tally.
#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// Photos the ledger had not seen when the run began.
    pub total: usize,
    pub uploaded: usize,
    /// Found in the library already.
    pub existing: usize,
    /// Unreadable, too big, or not exportable; tried again next run.
    pub skipped: usize,
    /// Not reached: the run was cancelled.
    pub pending: usize,
}
impl Outcome {
    pub fn message(&self) -> String {
        if self.total == 0 {
            return t("cloud.sync.up_to_date").into();
        }
        let mut message = match self.uploaded {
            0 => t("cloud.sync.summary_none").to_string(),
            n => schist_i18n::tn("cloud.sync.summary_uploaded", n as u64),
        };
        if self.existing > 0 {
            message.push_str("; ");
            message.push_str(&schist_i18n::tn(
                "cloud.sync.summary_existing",
                self.existing as u64,
            ));
        }
        if self.skipped > 0 {
            message.push_str("; ");
            message.push_str(&schist_i18n::tn(
                "cloud.sync.summary_skipped",
                self.skipped as u64,
            ));
        }
        if self.pending > 0 {
            message.push_str("; ");
            message.push_str(&schist_i18n::tn(
                "cloud.sync.summary_pending",
                self.pending as u64,
            ));
        }
        message
    }
}

/// One rule, ready to run against a connection.
pub struct Engine {
    pub source: Source,
    pub folder_id: Option<String>,
    /// Lower-case extensions that count as photos.
    pub extensions: HashSet<String>,
    pub ledger_path: PathBuf,
    pub ledger_key: String,
    pub staging: PathBuf,
    /// Read between photos; set, the run stops after the one in hand.
    pub cancel: Arc<AtomicBool>,
}
static RUNNING: AtomicBool = AtomicBool::new(false);
struct RunGuard;
impl Drop for RunGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}
impl Engine {
    /// Everything the source offers that the ledger has not seen.
    fn candidates(&self) -> Result<Vec<Candidate>> {
        match &self.source {
            Source::Folder { path } => {
                anyhow::ensure!(
                    path.is_dir(),
                    tf!("cloud.sync.source_unreadable", path = path.display())
                );
                list_folder(path, &self.extensions)
            }
            #[cfg(target_os = "ios")]
            Source::Album { id, .. } => ios::list_assets(id),
            #[cfg(not(target_os = "ios"))]
            Source::Album { .. } => Err(anyhow!(t("cloud.sync.albums_ios_only"))),
        }
    }
    /// Bring the cloud folder up to date. `report` hears the progress as
    /// the upload pipeline tells it: done, total, and what it is doing.
    pub async fn run(&self, handle: &remote::Handle, report: Report) -> Result<Outcome> {
        self.run_with(report, |files, report, handled| async move {
            let uploader = upload_files(
                handle,
                self.folder_id.clone(),
                files,
                report,
                Some(self.cancel.clone()),
                Some(handled),
            )
            .await?;
            Ok((
                uploader.uploaded.len(),
                uploader.existing.len(),
                uploader.skipped.len(),
            ))
        })
        .await
    }
    async fn run_with<F, Fut>(&self, report: Report, upload: F) -> Result<Outcome>
    where
        F: Fn(Vec<(PathBuf, Option<String>)>, Report, Handled) -> Fut,
        Fut: std::future::Future<Output = Result<(usize, usize, usize)>>,
    {
        schist_cloud_transfer::cancellable(Some(&self.cancel), async {
            while RUNNING
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                remote::runtime::sleep(Duration::from_millis(100)).await;
            }
            Ok(())
        })
        .await?;
        let _run = RunGuard;
        // A process killed by the OS cannot run its export cleanup. The
        // staging directory belongs to this engine and runs are serialized.
        if matches!(self.source, Source::Album { .. }) && self.staging.exists() {
            std::fs::remove_dir_all(&self.staging)?;
        }
        let mut ledger = Ledger::load(&self.ledger_path);
        if ledger.rule != self.ledger_key {
            ledger = Ledger {
                rule: self.ledger_key.clone(),
                ..Default::default()
            };
        }
        let todo: Vec<Candidate> = self
            .candidates()?
            .into_iter()
            .filter(|c| !ledger.seen.contains(&c.identity))
            .collect();
        let ledger = Arc::new(Mutex::new(ledger));
        let total = todo.len();
        let mut outcome = Outcome {
            total,
            ..Default::default()
        };
        report(
            0,
            total as u64,
            tf!("cloud.sync.progress", n = 0, m = total),
        );
        let mut base = 0u64;
        for chunk in todo.chunks(CHUNK) {
            if self.cancel.load(Ordering::Relaxed) {
                outcome.pending = total - base as usize;
                break;
            }
            let mut files = Vec::new();
            let mut staged = Vec::new();
            let mut by_path: HashMap<PathBuf, String> = HashMap::new();
            for candidate in chunk {
                if self.cancel.load(Ordering::Relaxed) {
                    break;
                }
                let path = match &candidate.locate {
                    Locate::Path(path) => path.clone(),
                    Locate::Asset(id) => {
                        match materialize(id, &candidate.name, &self.staging, &self.cancel) {
                            Ok(path) => {
                                staged.push(path.clone());
                                path
                            }
                            Err(error) => {
                                log::warn!(
                                    "camera sync: could not export {}: {error}",
                                    candidate.name
                                );
                                outcome.skipped += 1;
                                continue;
                            }
                        }
                    }
                };
                by_path.insert(path.clone(), candidate.identity.clone());
                files.push((path, candidate.relative.clone()));
            }
            let inner: Report = {
                let report = report.clone();
                Arc::new(move |done, _chunk_total, label| report(base + done, total as u64, label))
            };
            let handled: Handled = {
                let ledger = ledger.clone();
                let path = self.ledger_path.clone();
                Arc::new(move |paths| {
                    if paths.is_empty() {
                        return Ok(());
                    }
                    let mut ledger = ledger.lock().unwrap_or_else(|e| e.into_inner());
                    for source in paths {
                        if let Some(identity) = by_path.get(source) {
                            ledger.seen.insert(identity.clone());
                        }
                    }
                    ledger.save(&path)
                })
            };
            let result = upload(files, inner, handled).await;
            for path in &staged {
                let _ = std::fs::remove_file(path);
                // Each staged photo sits in a folder of its own.
                if let Some(parent) = path.parent() {
                    let _ = std::fs::remove_dir(parent);
                }
            }
            let (uploaded, existing, skipped) = result?;
            outcome.uploaded += uploaded;
            outcome.existing += existing;
            outcome.skipped += skipped;
            base += chunk.len() as u64;
        }
        report(base, total as u64, outcome.message());
        Ok(outcome)
    }
}

/// Write a Photos asset out for its upload. Only iOS has a Photos
/// library; anywhere else an album source is refused before this.
#[cfg(target_os = "ios")]
fn materialize(id: &str, name: &str, staging: &Path, cancel: &Arc<AtomicBool>) -> Result<PathBuf> {
    ios::export_original(id, name, staging, cancel)
}
#[cfg(not(target_os = "ios"))]
fn materialize(
    _id: &str,
    _name: &str,
    _staging: &Path,
    _cancel: &Arc<AtomicBool>,
) -> Result<PathBuf> {
    Err(anyhow!(t("cloud.sync.albums_ios_only")))
}

/// The codecs' extensions, less the document formats: a camera roll
/// holds photos, and a `.psd` or `.afphoto` found beside them is someone's
/// project, not a capture to back up.
pub fn photo_extensions(codec_extensions: Vec<String>) -> HashSet<String> {
    const DOCUMENTS: &[&str] = &["psd", "psb", "afphoto", "afdesign", "afpub", "af"];
    codec_extensions
        .into_iter()
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| !DOCUMENTS.contains(&e.as_str()))
        .collect()
}

/// The seconds since the epoch, for `last_run`.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A folder's "something changed" stamp: the newest mtime of the folder
/// and its immediate sub-folders — a new photo in `DCIM/Camera` touches
/// `Camera`, not `DCIM`. One `read_dir`, cheap enough every half minute.
pub fn folder_stamp(root: &Path) -> Option<u64> {
    let mtime = |path: &Path| {
        std::fs::metadata(path)
            .ok()?
            .modified()
            .ok()?
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    };
    let mut newest = mtime(root)?;
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if let Some(m) = mtime(&entry.path()) {
                    newest = newest.max(m);
                }
            }
        }
    }
    Some(newest)
}

#[cfg(target_os = "android")]
pub mod android;
#[cfg(target_os = "ios")]
pub mod ios;
#[cfg(any(target_os = "ios", target_os = "android"))]
use schist_app_settings::feature_enabled;
/// Credential identifier shared with GPUI and the headless backup job.
pub const CREDENTIAL_KEY: &str = "https://schist.app/schist-cloud";
pub fn state_dir() -> PathBuf {
    schist_gallery::state_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("schist/cloud")
}
pub fn parse_created_folder(reply: remote::Value) -> Result<String> {
    if let Some(id) = reply
        .as_map()
        .and_then(|entries| entries.iter().find(|(k, _)| k.as_str() == Some("id")))
        .and_then(|(_, v)| v.as_str())
        .filter(|id| !id.trim().is_empty())
    {
        return Ok(id.to_string());
    }
    Err(anyhow!(t("cloud.sync.folder_id_missing")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exts() -> HashSet<String> {
        ["jpg", "heic", "png"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn folder_listing_skips_hidden_symlinks_and_other_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("Camera/.thumbnails")).unwrap();
        std::fs::write(root.join("Camera/IMG_0001.JPG"), b"jpeg").unwrap();
        std::fs::write(root.join("Camera/VID_0001.mp4"), b"video").unwrap();
        std::fs::write(root.join("Camera/.nomedia"), b"").unwrap();
        std::fs::write(root.join("Camera/.thumbnails/x.jpg"), b"thumb").unwrap();
        std::fs::write(root.join("Screenshots.png"), b"png").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("Screenshots.png"), root.join("link.png")).unwrap();
        let listed = list_folder(root, &exts()).unwrap();
        let mut names: Vec<_> = listed.iter().map(|c| c.name.as_str()).collect();
        names.sort();
        assert_eq!(names, ["IMG_0001.JPG", "Screenshots.png"]);
        let camera = listed.iter().find(|c| c.name == "IMG_0001.JPG").unwrap();
        assert_eq!(camera.relative.as_deref(), Some("Camera/IMG_0001.JPG"));
        assert!(
            camera.identity.contains("|4"),
            "size is part of the identity"
        );
    }

    #[test]
    fn ledger_round_trips_and_survives_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/camera-sync.json");
        assert!(Ledger::load(&path).seen.is_empty());
        let mut ledger = Ledger::default();
        ledger.seen.insert("a|1|2".into());
        ledger.save(&path).unwrap();
        assert!(Ledger::load(&path).seen.contains("a|1|2"));
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn rule_serialises_with_defaults() {
        let rule: CameraSync = serde_json::from_str("{}").unwrap();
        assert!(!rule.enabled && !rule.asked && rule.source.is_none());
        let source = Source::Folder {
            path: PathBuf::from("/storage/emulated/0/DCIM/Camera"),
        };
        let text = serde_json::to_string(&source).unwrap();
        assert!(text.contains("\"kind\":\"folder\""));
        assert_eq!(serde_json::from_str::<Source>(&text).unwrap(), source);
    }

    #[test]
    fn folder_stamp_moves_with_a_subfolder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("Camera")).unwrap();
        let before = folder_stamp(root).unwrap();
        // mtimes are whole seconds on some filesystems: push the
        // sub-folder well past the root.
        let later = filetime::FileTime::from_unix_time(before as i64 + 60, 0);
        filetime::set_file_mtime(root.join("Camera"), later).unwrap();
        assert!(folder_stamp(root).unwrap() > before);
    }

    #[test]
    fn outcome_message_reads_well() {
        assert_eq!(
            Outcome::default().message(),
            t("cloud.sync.up_to_date").to_string()
        );
        let outcome = Outcome {
            total: 3,
            uploaded: 2,
            existing: 1,
            ..Default::default()
        };
        assert!(outcome.message().contains("; "));
    }

    fn engine(root: &Path, count: usize) -> Engine {
        let source = root.join("source");
        std::fs::create_dir(&source).unwrap();
        for n in 0..count {
            std::fs::write(source.join(format!("{n:04}.jpg")), [n as u8]).unwrap();
        }
        Engine {
            source: Source::Folder { path: source },
            folder_id: Some("destination".into()),
            extensions: exts(),
            ledger_path: root.join("ledger.json"),
            ledger_key: "rule-one".into(),
            staging: root.join("staging"),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
    fn run_test(work: impl std::future::Future<Output = ()>) {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(work);
    }
    fn report() -> Report {
        Arc::new(|_, _, _| {})
    }

    #[test]
    fn camera_sync_chunks_cancel_and_resume_without_reuploading() {
        run_test(async {
            let dir = tempfile::tempdir().unwrap();
            let engine = engine(dir.path(), CHUNK * 2 + 3);
            let cancel = engine.cancel.clone();
            let first = engine
                .run_with(report(), |files, _, handled| {
                    let cancel = cancel.clone();
                    async move {
                        assert_eq!(files.len(), CHUNK);
                        handled(&files.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>())?;
                        cancel.store(true, Ordering::Relaxed);
                        Ok((files.len(), 0, 0))
                    }
                })
                .await
                .unwrap();
            assert_eq!(first.uploaded, CHUNK);
            assert_eq!(first.pending, CHUNK + 3);
            engine.cancel.store(false, Ordering::Relaxed);
            let sizes = Mutex::new(Vec::new());
            let second = engine
                .run_with(report(), |files, _, handled| {
                    sizes.lock().unwrap().push(files.len());
                    async move {
                        handled(&files.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>())?;
                        Ok((files.len(), 0, 0))
                    }
                })
                .await
                .unwrap();
            assert_eq!(second.uploaded, CHUNK + 3);
            assert_eq!(*sizes.lock().unwrap(), [CHUNK, 3]);
            let third = engine
                .run_with(report(), |_, _, _| async {
                    panic!("known photos uploaded")
                })
                .await
                .unwrap();
            assert_eq!(third.total, 0);
        });
    }

    #[test]
    fn camera_sync_checkpoints_partial_failure_and_retries_unhandled_photos() {
        run_test(async {
            let dir = tempfile::tempdir().unwrap();
            let engine = engine(dir.path(), 3);
            let failed = engine
                .run_with(report(), |files, _, handled| async move {
                    handled(&[files[0].0.clone()])?; // duplicate or completed upload
                    anyhow::bail!("connection lost after acknowledgement")
                })
                .await;
            assert!(failed.is_err());
            assert_eq!(Ledger::load(&engine.ledger_path).seen.len(), 1);
            let retry = engine
                .run_with(report(), |files, _, handled| async move {
                    assert_eq!(files.len(), 2);
                    handled(&files.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>())?;
                    Ok((1, 1, 0))
                })
                .await
                .unwrap();
            assert_eq!((retry.uploaded, retry.existing), (1, 1));
        });
    }

    #[test]
    fn camera_sync_new_rule_does_not_reuse_another_destinations_ledger() {
        run_test(async {
            let dir = tempfile::tempdir().unwrap();
            let mut engine = engine(dir.path(), 1);
            for rule in ["first-account", "other-account"] {
                engine.ledger_key = rule.into();
                let result = engine
                    .run_with(report(), |files, _, handled| async move {
                        assert_eq!(files.len(), 1);
                        handled(&[files[0].0.clone()])?;
                        Ok((1, 0, 0))
                    })
                    .await
                    .unwrap();
                assert_eq!(result.total, 1);
            }
        });
    }

    #[test]
    fn camera_sync_folder_reply_requires_a_nonempty_id() {
        use remote::protocol::map;
        assert_eq!(
            parse_created_folder(map([("id", "folder-1".into())])).unwrap(),
            "folder-1"
        );
        assert!(parse_created_folder(map([("id", "".into())])).is_err());
        assert!(parse_created_folder(remote::Value::Nil).is_err());
    }

    #[test]
    fn camera_sync_cancellation_interrupts_a_pending_request() {
        run_test(async {
            let cancel = Arc::new(AtomicBool::new(false));
            let trigger = cancel.clone();
            let task = async move {
                remote::runtime::sleep(Duration::from_millis(20)).await;
                trigger.store(true, Ordering::Relaxed);
            };
            let waiting = schist_cloud_transfer::cancellable(
                Some(&cancel),
                futures::future::pending::<Result<()>>(),
            );
            let (_, result) = futures::join!(task, waiting);
            assert!(result
                .unwrap_err()
                .to_string()
                .contains(t("cloud.upload.cancelled")));
        });
    }
}
