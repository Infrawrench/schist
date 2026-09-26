//! The optional local Python backend is isolated from the editor process.
//! Installation is explicit; inference only reads pinned, installed weights.
use anyhow::{ensure, Context, Result};
use schist_i18n::t;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
const SCRIPT: &str = include_str!("../../../tools/image-to-3d.py");

pub fn installed(root: &Path) -> bool {
    root.join("ready.json").is_file()
        && python(root).is_file()
        && root.join("weights/model.ckpt").is_file()
        && root.join("weights/config.yaml").is_file()
        && root.join("dino/config.json").is_file()
        && root.join("source/tsr/system.py").is_file()
}
fn python(root: &Path) -> PathBuf {
    root.join(if cfg!(windows) {
        "venv/Scripts/python.exe"
    } else {
        "venv/bin/python"
    })
}
fn run(mut command: Command, cancel: &AtomicBool, timeout: Duration) -> Result<()> {
    ensure!(
        !cancel.load(Ordering::Relaxed),
        t("model3d.error.cancelled")
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let log = tempfile::tempfile()?;
    command
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log.try_clone()?));
    let mut child = command.spawn().context(t("model3d.error.python"))?;
    let start = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Relaxed) || start.elapsed() > timeout {
            // The installer starts pip and the weight downloader. Terminate
            // the whole job, so cancel cannot leave downloads running.
            #[cfg(unix)]
            unsafe {
                // A dedicated process group was created by process_group(0).
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            #[cfg(windows)]
            let _ = Command::new("taskkill")
                .args(["/PID", &child.id().to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!(t("model3d.error.cancelled"));
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        use std::io::{Read, Seek, SeekFrom};
        let mut log = log;
        let length = log.metadata()?.len();
        log.seek(SeekFrom::Start(length.saturating_sub(8192)))?;
        let mut bytes = Vec::new();
        log.read_to_end(&mut bytes)?;
        anyhow::bail!(
            "{}\n{}",
            t("model3d.error.reconstruction"),
            String::from_utf8_lossy(&bytes)
        );
    }
    Ok(())
}
pub fn install(root: &Path, cancel: &AtomicBool) -> Result<()> {
    let executable = std::env::var_os("SCHIST_3D_PYTHON").unwrap_or_else(|| {
        if cfg!(windows) {
            "python".into()
        } else {
            "python3".into()
        }
    });
    let mut command = Command::new(executable);
    command
        .args(["-c", SCRIPT, "--root"])
        .arg(root)
        .arg("install");
    run(command, cancel, Duration::from_secs(7200))?;
    ensure!(installed(root), t("model3d.error.install"));
    Ok(())
}
pub fn reconstruct(root: &Path, png: &[u8], cancel: &AtomicBool) -> Result<super::Mesh> {
    ensure!(installed(root), t("model3d.error.install"));
    let directory = tempfile::tempdir()?;
    let input = directory.path().join("input.png");
    let output = directory.path().join("mesh.glb");
    std::fs::write(&input, png)?;
    let mut command = Command::new(python(root));
    command
        .args(["-c", SCRIPT, "--root"])
        .arg(root)
        .args(["run", "--input"])
        .arg(input)
        .arg("--output")
        .arg(&output)
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1");
    run(command, cancel, Duration::from_secs(3600))?;
    ensure!(
        std::fs::metadata(&output)?.len() <= 128 * 1024 * 1024,
        t("model3d.error.limit")
    );
    super::import(&std::fs::read(output)?, "glb")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn cancellation_terminates_child_downloaders_too() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("orphaned-child");
        let mut command = Command::new("sh");
        command
            .args(["-c", "(sleep 0.4; echo orphaned > \"$1\") & wait", "test"])
            .arg(&marker);
        assert!(run(command, &AtomicBool::new(false), Duration::from_millis(30)).is_err());
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            !marker.exists(),
            "the installer child survived cancellation"
        );
    }
    #[test]
    fn already_cancelled_job_never_starts_and_logs_accept_partial_utf8() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("started");
        let mut command = Command::new("touch");
        command.arg(&marker);
        assert!(run(command, &AtomicBool::new(true), Duration::from_secs(1)).is_err());
        assert!(!marker.exists());
        let mut command = Command::new("sh");
        command.args(["-c", "printf '\\377diagnostic'; exit 1"]);
        let error = run(command, &AtomicBool::new(false), Duration::from_secs(1)).unwrap_err();
        assert!(error.to_string().contains("diagnostic"));
    }
}
