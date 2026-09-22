//! Remote shutter and original download through the installed gphoto2 client.
//! No shell, deletion command, or implicit replacement of existing originals.
use schist_i18n::t;
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Camera {
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
                .any(|c| c.is_control() || "/\\:%".contains(c))
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
    // OS/driver details are diagnostics; the surrounding message is localized.
    schist_i18n::tf!("library.ops.save_failed", error = error)
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(t("common.cancelled").into())
    } else {
        Ok(())
    }
}

/// Injectable process boundary. Each request is finite and owns a private cwd.
pub trait Runner {
    fn run(&self, args: &[OsString], cwd: &Path, cancel: &AtomicBool) -> Result<String>;
}
#[derive(Default)]
pub struct Gphoto;
impl Runner for Gphoto {
    fn run(&self, args: &[OsString], cwd: &Path, cancel: &AtomicBool) -> Result<String> {
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            return Err(t("common.not_available").into());
        }
        let mut command = Command::new("gphoto2");
        command
            .args(["--hook-script", "/usr/bin/true"])
            .args(args)
            .current_dir(cwd)
            .env("LC_ALL", "C");
        run_process(command, cancel, Duration::from_secs(120))
    }
}
fn run_process(mut command: Command, cancel: &AtomicBool, timeout: Duration) -> Result<String> {
    cancelled(cancel)?;
    let mut out = tempfile::tempfile().map_err(io_error)?;
    let mut err = tempfile::tempfile().map_err(io_error)?;
    command
        .stdin(Stdio::null())
        .stdout(out.try_clone().map_err(io_error)?)
        .stderr(err.try_clone().map_err(io_error)?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            t("tethered.install").into()
        } else {
            io_error(e)
        }
    })?;
    let mut child = ProcessGuard(child);
    let started = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Acquire) || started.elapsed() > timeout {
            return Err(if cancel.load(Ordering::Acquire) {
                t("common.cancelled").into()
            } else {
                t("tethered.timeout").into()
            });
        }
        match child.0.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(e) => return Err(io_error(e)),
        }
    };
    let read = |file: &mut File| -> Result<String> {
        file.seek(SeekFrom::Start(0)).map_err(io_error)?;
        let mut bytes = Vec::new();
        file.take(64 * 1024)
            .read_to_end(&mut bytes)
            .map_err(io_error)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    if !status.success() {
        return Err(schist_i18n::tf!(
            "tethered.failed",
            detail = read(&mut err)?
        ));
    }
    read(&mut out)
}
/// Kill/reap on cancellation, timeout, I/O errors and unwinding. The child has
/// its own process group, so unrelated camera applications are never signaled.
struct ProcessGuard(std::process::Child);
impl Drop for ProcessGuard {
    fn drop(&mut self) {
        // try_wait reaps on success; do not signal an already-reaped PID that
        // could have been reused by another process.
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
fn camera_args(camera: &Camera, operation: &str) -> Vec<OsString> {
    args(&["--camera", &camera.model, "--port", &camera.port, operation])
}
pub fn parse_cameras(output: &str) -> Vec<Camera> {
    output
        .lines()
        .filter_map(|line| {
            let (model, port) = line.trim_end().rsplit_once(char::is_whitespace)?;
            let model = model.trim();
            let port = port.trim();
            if model.is_empty() || !port.starts_with("usb:") {
                return None;
            }
            Some(Camera {
                model: model.into(),
                port: port.into(),
            })
        })
        .collect()
}
pub fn discover(runner: &impl Runner, cancel: &AtomicBool) -> Result<Vec<Camera>> {
    let cwd = tempfile::tempdir().map_err(io_error)?;
    let output = runner.run(&args(&["--auto-detect"]), cwd.path(), cancel)?;
    cancelled(cancel)?;
    Ok(parse_cameras(&output))
}
/// An abilities query opens no shutter. Only devices advertising Image capture
/// become usable; fixed C locale makes gphoto2's documented output stable.
pub fn supports_capture(output: &str) -> bool {
    let mut capture = false;
    for line in output.lines() {
        if let Some((key, value)) = line.split_once(':') {
            if !key.trim().is_empty() {
                capture = key.trim() == "Capture choices";
            }
            if capture && value.split_whitespace().any(|v| v == "Image") {
                return true;
            }
        }
    }
    false
}
pub fn connect(runner: &impl Runner, camera: &Camera, cancel: &AtomicBool) -> Result<()> {
    let cwd = tempfile::tempdir().map_err(io_error)?;
    let output = runner.run(&camera_args(camera, "--abilities"), cwd.path(), cancel)?;
    cancelled(cancel)?;
    if supports_capture(&output) {
        Ok(())
    } else {
        Err(t("tethered.unsupported").into())
    }
}

/// Paths already committed must always be imported, including a rare partial
/// publication failure. A warning never makes the caller discard saved images.
#[derive(Debug)]
pub struct Captured {
    pub paths: Vec<PathBuf>,
    pub warning: Option<String>,
}

pub fn capture(
    runner: &impl Runner,
    camera: &Camera,
    session: &Session,
    cancel: &AtomicBool,
) -> Result<Captured> {
    capture_with_publisher(runner, camera, session, cancel, |source, dest| {
        source.persist_noclobber(dest)
    })
}
fn capture_with_publisher(
    runner: &impl Runner,
    camera: &Camera,
    session: &Session,
    cancel: &AtomicBool,
    mut publish: impl FnMut(
        tempfile::TempPath,
        &Path,
    ) -> std::result::Result<(), tempfile::PathPersistError>,
) -> Result<Captured> {
    session.validate()?;
    cancelled(cancel)?;
    // Staging is on the destination filesystem, permitting no-replace rename
    // publication even on volumes without hard links (for example exFAT).
    let staging = tempfile::Builder::new()
        .prefix(".schist-capture-")
        .tempdir_in(&session.destination)
        .map_err(io_error)?;
    let mut command = camera_args(camera, "--keep");
    command.extend(args(&[
        "--filename",
        "capture-%06n.%C",
        "--capture-image-and-download",
    ]));
    runner.run(&command, staging.path(), cancel)?;
    cancelled(cancel)?;
    let mut sources = fs::read_dir(staging.path())
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
        File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(io_error)?;
    }
    cancelled(cancel)?;
    // Commit boundary: finish the complete set once publication starts. Caller
    // must import these paths even if Cancel arrived during this short commit.
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
        // One shared free sequence keeps RAW+JPEG together even when only one
        // extension of an earlier exposure already exists. A dangling symlink
        // counts as occupied too; persist_noclobber handles late races.
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
                    // Never roll back completed originals: after commit starts,
                    // even an unusual external collision/disk failure returns
                    // its actual saved paths for exactly-once gallery import.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Fake {
        files: Vec<(&'static str, &'static [u8])>,
        output: &'static str,
        cancel_during: bool,
        fail: bool,
        calls: Mutex<Vec<Vec<OsString>>>,
    }
    impl Runner for Fake {
        fn run(&self, args: &[OsString], cwd: &Path, cancel: &AtomicBool) -> Result<String> {
            self.calls.lock().unwrap().push(args.to_vec());
            for (name, bytes) in &self.files {
                fs::write(cwd.join(name), bytes).unwrap();
            }
            if self.cancel_during {
                cancel.store(true, Ordering::Release);
            }
            if self.fail {
                Err("disconnected".into())
            } else {
                Ok(self.output.into())
            }
        }
    }
    fn camera() -> Camera {
        Camera {
            model: "Camera $(not-a-command)".into(),
            port: "usb:001,002".into(),
        }
    }
    fn session(dir: &Path) -> Session {
        Session {
            destination: dir.to_path_buf(),
            prefix: "shoot ' $()".into(),
        }
    }
    #[test]
    fn parse_detection_and_capture_abilities() {
        assert_eq!(parse_cameras("Model                          Port\n----------------------------------------------------------\nCanon EOS 80D                   usb:001,005\nNikon Z 6                      usb:002,009    \n"), vec![
            Camera { model: "Canon EOS 80D".into(), port: "usb:001,005".into() },
            Camera { model: "Nikon Z 6".into(), port: "usb:002,009".into() },
        ]);
        assert!(parse_cameras("Model Port\n---------\n").is_empty());
        assert!(supports_capture("Capture choices                  :\n                                 : Image\n                                 : Preview\n"));
        assert!(supports_capture("Capture choices : Image\n"));
        assert!(!supports_capture(
            "Capture choices : Preview\nFile operations : Image\n"
        ));
    }
    #[test]
    fn originals_publish_without_overwrite_and_arguments_are_not_shell_text() {
        let dir = tempfile::tempdir().unwrap();
        let session = session(dir.path());
        let existing = dir.path().join(format!("{}-000001.jpg", session.prefix));
        fs::write(&existing, b"original").unwrap();
        let fake = Fake {
            files: vec![
                ("capture-000001.JPG", b"jpeg"),
                ("capture-000002.CR3", b"raw"),
            ],
            ..Default::default()
        };
        let result = capture(&fake, &camera(), &session, &AtomicBool::new(false)).unwrap();
        assert!(result.warning.is_none());
        let paths = result.paths;
        assert_eq!(paths.len(), 2);
        assert_eq!(fs::read(existing).unwrap(), b"original");
        assert!(paths.contains(&dir.path().join(format!("{}-000002.jpg", session.prefix))));
        assert!(paths.contains(&dir.path().join(format!("{}-000002.cr3", session.prefix))));
        let calls = fake.calls.lock().unwrap();
        assert_eq!(
            calls[0],
            args(&[
                "--camera",
                &camera().model,
                "--port",
                &camera().port,
                "--keep",
                "--filename",
                "capture-%06n.%C",
                "--capture-image-and-download"
            ])
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }
    #[test]
    fn late_collision_retries_whole_set_or_reports_committed_files() {
        for collision_at in [0, 1] {
            let dir = tempfile::tempdir().unwrap();
            let session = session(dir.path());
            let fake = Fake {
                files: vec![("capture-1.jpg", b"jpeg"), ("capture-2.cr3", b"raw")],
                ..Default::default()
            };
            let mut calls = 0;
            let mut foreign = None;
            let result = capture_with_publisher(
                &fake,
                &camera(),
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
                assert!(result.paths.iter().all(|p| p
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
    fn cancellation_failure_empty_or_partial_download_never_imports() {
        for fake in [
            Fake {
                files: vec![("capture.jpg", b"photo")],
                cancel_during: true,
                ..Default::default()
            },
            Fake {
                files: vec![("capture.jpg", b"partial")],
                fail: true,
                ..Default::default()
            },
            Fake {
                files: vec![("capture.jpg", b"")],
                ..Default::default()
            },
            Fake::default(),
        ] {
            let dir = tempfile::tempdir().unwrap();
            assert!(capture(
                &fake,
                &camera(),
                &session(dir.path()),
                &AtomicBool::new(false)
            )
            .is_err());
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        }
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        assert!(capture(
            &fake,
            &camera(),
            &session(dir.path()),
            &AtomicBool::new(true)
        )
        .is_err());
        assert!(fake.calls.lock().unwrap().is_empty());
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
        ] {
            session.prefix = bad.into();
            assert!(session.validate().is_err(), "{bad:?}");
        }
        fs::write(path, "{broken").unwrap();
        assert!(Session::load(&dir.path().join("settings.json")).is_err());
    }
    #[test]
    fn connect_is_capability_gated_and_retries_are_independent() {
        let cancel = AtomicBool::new(false);
        assert!(connect(&Fake::default(), &camera(), &cancel).is_err());
        let fake = Fake {
            output: "Capture choices : Image",
            ..Default::default()
        };
        connect(&fake, &camera(), &cancel).unwrap();
        cancel.store(true, Ordering::Release);
        assert!(connect(&fake, &camera(), &cancel).is_err());
        connect(&fake, &camera(), &AtomicBool::new(false)).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlink_download_is_rejected() {
        struct LinkFake;
        impl Runner for LinkFake {
            fn run(&self, _: &[OsString], cwd: &Path, _: &AtomicBool) -> Result<String> {
                std::os::unix::fs::symlink("/etc/passwd", cwd.join("capture.jpg")).unwrap();
                Ok(String::new())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(capture(
            &LinkFake,
            &camera(),
            &session(dir.path()),
            &AtomicBool::new(false)
        )
        .is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    #[test]
    fn process_output_and_failure_are_distinguished() {
        let cancel = AtomicBool::new(false);
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf 'Camera usb:001,002\\n'"]);
        assert_eq!(
            run_process(command, &cancel, Duration::from_secs(2)).unwrap(),
            "Camera usb:001,002\n"
        );
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf 'camera unplugged' >&2; exit 1"]);
        let error = run_process(command, &cancel, Duration::from_secs(2)).unwrap_err();
        assert!(error.contains("camera unplugged"), "{error:?}");
        assert!(run_process(
            Command::new("/nonexistent/schist-gphoto2"),
            &cancel,
            Duration::from_secs(2)
        )
        .is_err());
    }
    #[test]
    fn timeout_kills_descendants_before_they_can_write() {
        let dir = tempfile::tempdir().unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .current_dir(dir.path())
            .args(["-c", "(sleep 0.5; touch escaped) & wait"]);
        assert!(run_process(command, &AtomicBool::new(false), Duration::from_millis(80)).is_err());
        std::thread::sleep(Duration::from_millis(600));
        assert!(!dir.path().join("escaped").exists());
    }
    #[test]
    fn active_cancellation_reaps_the_process() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let signal = cancel.clone();
        let task = std::thread::spawn(move || {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", "sleep 20"]);
            run_process(command, &signal, Duration::from_secs(25))
        });
        std::thread::sleep(Duration::from_millis(80));
        cancel.store(true, Ordering::Release);
        let started = Instant::now();
        assert_eq!(task.join().unwrap().unwrap_err(), t("common.cancelled"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
