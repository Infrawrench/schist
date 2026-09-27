//! Prepare installed background models without delaying the app's event loop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{Builder, JoinHandle};
use std::time::Instant;

static STARTED: AtomicBool = AtomicBool::new(false);
static FOREGROUND_STARTED: AtomicBool = AtomicBool::new(false);

pub(super) fn foreground_started(id: &str) {
    if matches!(
        id,
        "foreground" | "foreground-matting" | "subject-guide" | "detail-matting" | "matting"
    ) {
        FOREGROUND_STARTED.store(true, Ordering::Relaxed);
    }
}

fn models(installed: impl Fn(&str) -> bool) -> Vec<&'static str> {
    if !installed("foreground") {
        return Vec::new();
    }
    let mut ids = Vec::new();
    // Only preload sessions supported by the desktop idle caches.
    #[cfg(any(
        target_os = "linux",
        target_os = "windows",
        all(target_os = "macos", target_arch = "aarch64")
    ))]
    {
        if installed("foreground-matting") {
            ids.push("foreground-matting");
        }
        ids.push("foreground");
    }
    ids.extend(["subject-guide", "matting", "detail-matting"]);
    ids
}

/// Start one background loading thread at GUI startup. Only already installed
/// background models are used; this never downloads weights or reads photos.
/// macOS warms resident GPU sessions; iOS only verifies/extracts compiled files
/// to avoid retaining models or submitting GPU work at startup.
/// Linux/Windows prepare CPU/GPU plans without running inference at startup.
/// The caller must drop the handle without joining to keep startup nonblocking.
/// Desktop sessions retain their normal five-minute idle eviction policy.
pub fn preload_background_removal() -> Option<JoinHandle<()>> {
    start_once(&STARTED, || {
        let start = Instant::now();
        let mut count = 0;
        for id in models(crate::installed) {
            // An actual action takes priority. Finish the current shared
            // preparation, then let its worker prepare remaining models itself.
            if FOREGROUND_STARTED.load(Ordering::Relaxed) {
                break;
            }
            count += usize::from(prepare(id));
        }
        log::info!(
            "background model preload: {count} models in {:.3}s",
            start.elapsed().as_secs_f64()
        );
    })
}

#[cfg(target_os = "ios")]
fn prepare(id: &str) -> bool {
    crate::native_coreml::prepare_bundled(id)
        .map_err(|error| log::warn!("background model {id} preparation: {error:#}"))
        .is_ok()
}

#[cfg(target_os = "macos")]
fn prepare(id: &str) -> bool {
    crate::with_adaptive_execution(|| {
        let loaded = crate::get_prepared(id, |model| {
            if let Err(error) = warm(model) {
                // Failed warm-up retains the ordinary prediction fallback.
                log::warn!("background model {id} warm-up: {error:#}");
            }
        });
        crate::release(id);
        loaded.is_some()
    })
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn prepare(id: &str) -> bool {
    crate::with_adaptive_execution(|| {
        let loaded = crate::get_prepared(id, |_| {});
        crate::release(id);
        loaded.is_some()
    })
}

fn start_once(
    started: &'static AtomicBool,
    run: impl FnOnce() + Send + 'static,
) -> Option<JoinHandle<()>> {
    if started.swap(true, Ordering::Relaxed) {
        return None;
    }
    match Builder::new()
        .name("background-model-preload".into())
        .spawn(run)
    {
        Ok(thread) => Some(thread),
        Err(error) => {
            started.store(false, Ordering::Relaxed);
            log::warn!("background model preload thread: {error}");
            None
        }
    }
}

#[cfg(target_os = "macos")]
fn warm(model: &crate::Model) -> anyhow::Result<()> {
    let (w, h) = model.input_dims();
    if model.channels() == 3 {
        model.run(&vec![0.5; w * h * 3])?;
    } else {
        let colors = vec![0.5; w * h];
        // A valid three-level trimap exercises the same graph as an edge tile.
        let trimap: Vec<_> = (0..w * h)
            .map(|i| ((i % w) * 3 / w).min(2) as f32 * 0.5)
            .collect();
        model.run_planes(&[&colors, &colors, &colors, &trimap])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn startup_returns_before_work_finishes_and_starts_only_once() {
        static START: AtomicBool = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel();
        let worker = start_once(&START, move || rx.recv().unwrap()).unwrap();
        assert!(!worker.is_finished());
        assert!(start_once(&START, || panic!("duplicate worker")).is_none());
        tx.send(()).unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn startup_only_prepares_an_installed_background_pipeline() {
        assert!(models(|_| false).is_empty());
        assert!(models(|id| id != "foreground").is_empty());
        #[cfg(target_os = "ios")]
        assert_eq!(
            models(|_| true),
            ["subject-guide", "matting", "detail-matting"]
        );
        let general = models(|id| id == "foreground");
        assert!(general.contains(&"detail-matting"));
        assert!(!general.contains(&"foreground-matting"));
        #[cfg(any(
            target_os = "linux",
            target_os = "windows",
            all(target_os = "macos", target_arch = "aarch64")
        ))]
        assert_eq!(
            models(|_| true),
            [
                "foreground-matting",
                "foreground",
                "subject-guide",
                "matting",
                "detail-matting"
            ]
        );
    }
}
