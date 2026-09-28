//! Opt-in native placement for the expensive, partially accelerated matting graphs.
//! Keep placement outside the model cache so idle eviction preserves timings.
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
    crate::cpu_threads::run(id, || run_cpu_inner(id, plan, inputs))
}

fn run_cpu_inner(
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
    // Android's OS-key TLS macro expands this const initializer through a
    // non-const helper, which triggers a false positive in Clippy 1.98.
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local))]
    static ADAPTIVE: Cell<bool> = const { Cell::new(false) };
}

pub(super) fn adaptive_enabled() -> bool {
    ADAPTIVE.get()
}

/// Warm GPU, then time CPU and GPU on successive real inputs of each background
/// model family and shape, reusing the faster placement for this backend. No
/// input is evaluated twice unless execution fails and needs the other backend.
/// Only affects synchronous inference inside `run` on the calling thread; do
/// not return a future and expect the setting to follow it across executors.
/// Other models and the global effects backend are unchanged.
pub fn with_adaptive_execution<T>(run: impl FnOnce() -> T) -> T {
    adaptive_scope(cfg!(any(target_os = "ios", target_os = "android")), run)
}

fn adaptive_scope<T>(mobile: bool, run: impl FnOnce() -> T) -> T {
    // Multiple windows must not hold multiple phone-sized inference pipelines.
    // Nested calls on this thread inherit the outer guard rather than deadlock.
    static MOBILE_INFERENCE: Mutex<()> = Mutex::new(());
    let _mobile = (mobile && !ADAPTIVE.get()).then(|| {
        MOBILE_INFERENCE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    });
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    // The first GPU output has already been used. Time CPU on the next actual
    // input (another edge tile or detector pass), without repeating either.
    ProbeCpu,
    // The cold GPU prediction compiled shaders and uploaded constants. Give
    // its warmed path a fair comparison before permanently selecting the CPU.
    ProbeGpu(Duration),
    Selected(bool),
}
struct Choice {
    backend: Weak<dyn FxBackend>,
    placement: Placement,
}
impl Choice {
    fn for_backend(&self, backend: &Arc<dyn FxBackend>) -> Option<Placement> {
        self.backend
            .upgrade()
            .filter(|previous| Arc::ptr_eq(previous, backend))
            .map(|_| self.placement)
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
    if let Some(Placement::Selected(use_gpu)) = choice {
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

    // Calibration must do useful work. In particular, a cold removal must not
    // pay for a second complete detector and attention pass merely to time it.
    // These fixed-shape graphs have the same work for each input. Concurrent
    // callers may repeat a probe, but never hold the cache lock during inference.
    let start = Instant::now();
    let attempt = match choice {
        Some(Placement::ProbeCpu) => match cpu() {
            Ok(result) => Ok((result, Placement::ProbeGpu(start.elapsed()))),
            Err(error) => gpu()
                .map(|result| (result, Placement::Selected(true)))
                .map_err(|_| error),
        },
        Some(Placement::ProbeGpu(cpu_time)) => match gpu() {
            Ok(result) => {
                let gpu_time = start.elapsed();
                let use_gpu = prefer_gpu(cpu_time, gpu_time);
                log::info!(
                    "neural placement {} {:?}: CPU {:.3}s, accelerated {:.3}s; using {}",
                    key.family,
                    key.shape,
                    cpu_time.as_secs_f64(),
                    gpu_time.as_secs_f64(),
                    if use_gpu { "GPU" } else { "CPU" },
                );
                Ok((result, Placement::Selected(use_gpu)))
            }
            Err(_) => cpu().map(|result| (result, Placement::Selected(false))),
        },
        _ => match gpu() {
            Ok(result) => Ok((result, Placement::ProbeCpu)),
            Err(_) => cpu().map(|result| (result, Placement::Selected(false))),
        },
    };
    let (result, placement) = match attempt {
        Ok(success) => success,
        Err(error) => {
            cache.lock().unwrap().remove(&key);
            return Err(error);
        }
    };
    cache.lock().unwrap().insert(
        key,
        Choice {
            backend: Arc::downgrade(&backend),
            placement,
        },
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_jobs_serialize_nest_and_recover_from_panics() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let active = AtomicUsize::new(0);
        let barrier = std::sync::Barrier::new(4);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    barrier.wait();
                    adaptive_scope(true, || {
                        assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                        adaptive_scope(true, std::thread::yield_now);
                        assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                    });
                });
            }
        });
        assert!(
            std::panic::catch_unwind(|| adaptive_scope(true, || panic!("test unwind"))).is_err()
        );
        adaptive_scope(true, || assert!(adaptive_enabled()));
        assert!(!adaptive_enabled());
    }

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
    fn calibration_requires_a_clear_cpu_win() {
        let cpu = Duration::from_secs(10);
        assert!(!prefer_gpu(cpu, Duration::from_secs(20)));
        assert!(prefer_gpu(cpu, Duration::from_secs(11)));
        assert!(prefer_gpu(cpu, Duration::from_secs(5)));
    }

    #[test]
    fn calibration_uses_each_input_once_and_returns_its_own_output() {
        let backend: Arc<dyn FxBackend> = Arc::new(schist_fx::CpuFx);
        let key = Key {
            family: "test-incremental",
            shape: vec![19],
        };
        assert_eq!(
            run(
                key.clone(),
                backend.clone(),
                || panic!("duplicate inference"),
                || Ok(1)
            )
            .unwrap(),
            1
        );
        assert!(matches!(
            CHOICES.get().unwrap().lock().unwrap()[&key].placement,
            Placement::ProbeCpu
        ));
        assert_eq!(
            run(
                key.clone(),
                backend.clone(),
                || Ok(2),
                || panic!("duplicate inference")
            )
            .unwrap(),
            2
        );
        assert!(matches!(
            CHOICES.get().unwrap().lock().unwrap()[&key].placement,
            Placement::ProbeGpu(_)
        ));
        assert_eq!(
            run(
                key.clone(),
                backend,
                || panic!("duplicate inference"),
                || Ok(3)
            )
            .unwrap(),
            3
        );
        assert!(matches!(
            CHOICES.get().unwrap().lock().unwrap()[&key].placement,
            Placement::Selected(_)
        ));
    }

    #[test]
    fn failed_cpu_probe_falls_back_and_failed_pair_can_retry() {
        let backend: Arc<dyn FxBackend> = Arc::new(schist_fx::CpuFx);
        let key = Key {
            family: "test-probe-failure",
            shape: vec![23],
        };
        run(
            key.clone(),
            backend.clone(),
            || panic!("duplicate"),
            || Ok(1),
        )
        .unwrap();
        assert_eq!(
            run(
                key.clone(),
                backend.clone(),
                || anyhow::bail!("CPU failed"),
                || Ok(2)
            )
            .unwrap(),
            2
        );
        assert_eq!(
            run(
                key.clone(),
                backend.clone(),
                || panic!("cached GPU"),
                || Ok(3)
            )
            .unwrap(),
            3
        );
        assert!(run::<()>(
            key.clone(),
            backend.clone(),
            || anyhow::bail!("CPU failed"),
            || anyhow::bail!("GPU failed")
        )
        .is_err());
        assert!(!CHOICES.get().unwrap().lock().unwrap().contains_key(&key));
        assert_eq!(
            run(key, backend, || Ok(4), || anyhow::bail!("GPU failed")).unwrap(),
            4
        );
    }

    #[test]
    fn failed_warm_gpu_probe_retries_cpu_without_losing_the_input() {
        let backend: Arc<dyn FxBackend> = Arc::new(schist_fx::CpuFx);
        let key = Key {
            family: "test-warm-probe-failure",
            shape: vec![29],
        };
        run(
            key.clone(),
            backend.clone(),
            || panic!("duplicate"),
            || Ok(1),
        )
        .unwrap();
        run(
            key.clone(),
            backend.clone(),
            || Ok(2),
            || panic!("duplicate"),
        )
        .unwrap();
        assert_eq!(
            run(
                key.clone(),
                backend.clone(),
                || Ok(3),
                || anyhow::bail!("device lost")
            )
            .unwrap(),
            3
        );
        assert_eq!(
            run(
                key,
                backend,
                || Ok(4),
                || panic!("failed GPU should not be selected")
            )
            .unwrap(),
            4
        );
    }

    #[test]
    fn backend_replacement_invalidates_cached_placement_without_retaining_device() {
        let backend: Arc<dyn FxBackend> = Arc::new(schist_fx::CpuFx);
        let replacement: Arc<dyn FxBackend> = Arc::new(schist_fx::CpuFx);
        let choice = Choice {
            backend: Arc::downgrade(&backend),
            placement: Placement::Selected(false),
        };
        assert_eq!(
            choice.for_backend(&backend),
            Some(Placement::Selected(false))
        );
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
