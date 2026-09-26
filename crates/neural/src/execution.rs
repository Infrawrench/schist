//! Opt-in native placement for the expensive, partially accelerated matting graphs.
//! Keep the choice outside the model cache: the layer worker releases each plan.
use anyhow::Result;
use schist_fx::FxBackend;
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};
use tract_onnx::prelude::*;

/// Opt-in development timings for the first CPU input of each model. Normal
/// inference does not install a per-node callback or retain tensor contents.
pub(super) fn run_cpu(
    id: &str,
    plan: &Arc<TypedSimplePlan>,
    inputs: TVec<TValue>,
) -> Result<TVec<TValue>> {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static SEEN: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("SCHIST_MATTING_PROFILE").is_some())
        || !SEEN
            .get_or_init(Mutex::default)
            .lock()
            .unwrap()
            .insert(id.to_owned())
    {
        return plan.run(inputs);
    }
    let mut timing: HashMap<String, (usize, Duration)> = HashMap::new();
    let detailed = std::env::var_os("SCHIST_MATTING_PROFILE_NODES").is_some();
    let mut nodes = Vec::new();
    let output = plan
        .spawn()?
        .run_plan_with_eval(inputs, |state, op_state, node, inputs| {
            let shapes = detailed.then(|| {
                inputs
                    .iter()
                    .map(|v| v.shape().to_vec())
                    .collect::<Vec<_>>()
            });
            let start = Instant::now();
            let output = tract_onnx::tract_core::plan::eval(state, op_state, node, inputs);
            let elapsed = start.elapsed();
            let entry = timing.entry(node.op().name().into_owned()).or_default();
            entry.0 += 1;
            entry.1 += elapsed;
            if let Some(shapes) = shapes {
                nodes.push((
                    elapsed,
                    node.name.clone(),
                    node.op().name().into_owned(),
                    shapes,
                ));
            }
            output
        })?;
    let mut timing = timing.into_iter().collect::<Vec<_>>();
    timing.sort_by_key(|(_, (_, elapsed))| std::cmp::Reverse(*elapsed));
    for (op, (calls, elapsed)) in timing.into_iter().take(15) {
        log::info!(
            "CPU profile {id}: {op} {calls} calls {:.3}s",
            elapsed.as_secs_f64()
        );
    }
    nodes.sort_by_key(|(elapsed, ..)| std::cmp::Reverse(*elapsed));
    for (elapsed, name, op, shapes) in nodes.into_iter().take(40) {
        log::info!(
            "CPU node {id}: {op} {name} {shapes:?} {:.6}s",
            elapsed.as_secs_f64()
        );
    }
    Ok(output)
}

thread_local! {
    static ADAPTIVE: Cell<bool> = const { Cell::new(false) };
}

/// Measure CPU and accelerated execution once per background-model family and
/// input shape, then reuse the faster placement for this backend's lifetime.
/// Only affects synchronous inference inside `run` on the calling thread; do
/// not return a future and expect the setting to follow it across executors.
/// Other models and the global effects backend are unchanged.
pub fn with_adaptive_execution<T>(run: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            ADAPTIVE.set(self.0);
        }
    }
    let _restore = Restore(ADAPTIVE.replace(true));
    run()
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub(super) struct Key {
    family: &'static str,
    shape: Vec<usize>,
}

impl Key {
    pub(super) fn for_model(id: &str, shape: &[usize]) -> Option<Self> {
        if !ADAPTIVE.get() {
            return None;
        }
        let family = match id {
            // These pinned exports share the BiRefNet Lite architecture and
            // input size. Reuse calibration for the second detector pass.
            "foreground" | "foreground-matting" => "birefnet-lite",
            "detail-matting" => "vitmatte-s",
            _ => return None,
        };
        Some(Self {
            family,
            shape: shape.to_vec(),
        })
    }
}

struct Choice {
    backend: Weak<dyn FxBackend>,
    gpu: bool,
}
impl Choice {
    fn for_backend(&self, backend: &Arc<dyn FxBackend>) -> Option<bool> {
        self.backend
            .upgrade()
            .filter(|previous| Arc::ptr_eq(previous, backend))
            .map(|_| self.gpu)
    }
}
static CHOICES: OnceLock<Mutex<HashMap<Key, Choice>>> = OnceLock::new();

// Require a clear CPU win; small timing fluctuations should not disable GPU
// offloading. Include uploads, readbacks and host operators in the measurement.
fn prefer_gpu(cpu: Duration, gpu: Duration) -> bool {
    cpu.as_secs_f64() * 1.2 >= gpu.as_secs_f64()
}

pub(super) fn run<T>(
    key: Key,
    backend: Arc<dyn FxBackend>,
    cpu: impl FnOnce() -> Result<T>,
    gpu: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let cache = CHOICES.get_or_init(Mutex::default);
    let choice = cache
        .lock()
        .unwrap()
        .get(&key)
        .and_then(|choice| choice.for_backend(&backend));
    if let Some(use_gpu) = choice {
        // A device failure may make yesterday's winning placement unusable.
        // Invalidate it and retain the other result if it succeeds.
        let mut failed = false;
        let result = if use_gpu {
            gpu().or_else(|_| {
                failed = true;
                cpu()
            })
        } else {
            cpu().or_else(|_| {
                failed = true;
                gpu()
            })
        };
        if failed {
            cache.lock().unwrap().remove(&key);
        }
        return result;
    }

    // Do not hold the cache lock while executing either graph. Concurrent
    // workers may calibrate independently rather than blocking one another.
    let start = Instant::now();
    let reference = cpu();
    let cpu_time = start.elapsed();
    let start = Instant::now();
    let accelerated = gpu();
    let gpu_time = start.elapsed();
    let (result, use_gpu) = select(reference, accelerated, cpu_time, gpu_time)?;
    log::info!(
        "neural placement {} {:?}: CPU {:.3}s, accelerated {:.3}s; using {}",
        key.family,
        key.shape,
        cpu_time.as_secs_f64(),
        gpu_time.as_secs_f64(),
        if use_gpu { "GPU" } else { "CPU" },
    );
    cache.lock().unwrap().insert(
        key,
        Choice {
            backend: Arc::downgrade(&backend),
            gpu: use_gpu,
        },
    );
    Ok(result)
}

fn select<T>(
    cpu: Result<T>,
    gpu: Result<T>,
    cpu_time: Duration,
    gpu_time: Duration,
) -> Result<(T, bool)> {
    match (cpu, gpu) {
        (Ok(cpu), Ok(gpu)) => Ok(if prefer_gpu(cpu_time, gpu_time) {
            (gpu, true)
        } else {
            (cpu, false)
        }),
        (Ok(cpu), Err(_)) => Ok((cpu, false)),
        (Err(_), Ok(gpu)) => Ok((gpu, true)),
        (Err(cpu), Err(_)) => Err(cpu),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_is_scoped_and_only_shared_by_matching_graphs() {
        let key = |id| Key::for_model(id, &[1, 3, 1024, 1024]);
        assert!(key("foreground").is_none());
        with_adaptive_execution(|| {
            assert_eq!(key("foreground"), key("foreground-matting"));
            assert_ne!(key("foreground"), key("detail-matting"));
            assert!(key("subject-guide").is_none());
            assert_ne!(
                key("foreground"),
                Key::for_model("foreground", &[1, 3, 512, 512])
            );
            with_adaptive_execution(|| assert!(key("foreground").is_some()));
            assert!(key("foreground").is_some());
        });
        assert!(key("foreground").is_none());
        let _ = std::panic::catch_unwind(|| with_adaptive_execution(|| panic!("test unwind")));
        assert!(key("foreground").is_none());
    }

    #[test]
    fn calibration_keeps_successful_results_and_requires_a_clear_cpu_win() {
        let cpu = Duration::from_secs(10);
        assert!(!prefer_gpu(cpu, Duration::from_secs(20)));
        assert!(prefer_gpu(cpu, Duration::from_secs(11)));
        assert!(prefer_gpu(cpu, Duration::from_secs(5)));
        assert_eq!(select(Ok(1), Ok(2), cpu, cpu).unwrap(), (2, true));
        assert_eq!(
            select(Ok(1), Err(anyhow::anyhow!("device lost")), cpu, cpu).unwrap(),
            (1, false)
        );
        assert_eq!(
            select(Err(anyhow::anyhow!("CPU failed")), Ok(2), cpu, cpu).unwrap(),
            (2, true)
        );
        assert!(select::<()>(
            Err(anyhow::anyhow!("CPU failed")),
            Err(anyhow::anyhow!("GPU failed")),
            cpu,
            cpu
        )
        .is_err());
    }

    #[test]
    fn backend_replacement_invalidates_cached_placement_without_retaining_device() {
        let backend: Arc<dyn FxBackend> = Arc::new(schist_fx::CpuFx);
        let replacement: Arc<dyn FxBackend> = Arc::new(schist_fx::CpuFx);
        let choice = Choice {
            backend: Arc::downgrade(&backend),
            gpu: false,
        };
        assert_eq!(choice.for_backend(&backend), Some(false));
        assert_eq!(choice.for_backend(&replacement), None);
        drop(backend);
        assert!(choice.backend.upgrade().is_none());
    }

    #[test]
    fn subsequent_tiles_reuse_placement_and_retry_failed_execution() {
        let backend: Arc<dyn FxBackend> = Arc::new(schist_fx::CpuFx);
        let key = Key {
            family: "test-cache",
            shape: vec![7, 13],
        };
        // A failed GPU calibration chooses CPU deterministically, without
        // sleeping or relying on wall-clock timing in a unit test.
        assert_eq!(
            run(
                key.clone(),
                backend.clone(),
                || Ok(7),
                || { Err(anyhow::anyhow!("device lost")) }
            )
            .unwrap(),
            7
        );
        assert_eq!(
            run(
                key.clone(),
                backend.clone(),
                || Ok(8),
                || { panic!("cached CPU choice should avoid the GPU") }
            )
            .unwrap(),
            8
        );
        assert_eq!(
            run(
                key.clone(),
                backend,
                || Err(anyhow::anyhow!("CPU failed")),
                || Ok(9)
            )
            .unwrap(),
            9
        );
        assert!(!CHOICES.get().unwrap().lock().unwrap().contains_key(&key));
    }
}
