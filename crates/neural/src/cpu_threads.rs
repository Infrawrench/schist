//! Bounded native tensor parallelism, scoped to the background models.
//! Share workers across plans/tiles; leave unrelated filters and Web alone.
use std::sync::{Arc, OnceLock};
use tract_linalg::multithread::{multithread_tract_scope, Executor};

fn thread_count(available: usize, mobile: bool, override_count: Option<usize>) -> usize {
    let limit = if mobile { 2 } else { 4 };
    override_count
        .unwrap_or(limit)
        .clamp(1, 8)
        .min(available.max(1))
}

fn executor() -> &'static Executor {
    static EXECUTOR: OnceLock<Executor> = OnceLock::new();
    EXECUTOR.get_or_init(|| {
        // The override also lets maintainers compare this portable path on a
        // Mac. Apple's production path keeps Core ML/Accelerate scheduling.
        let override_count = std::env::var("SCHIST_NEURAL_CPU_THREADS")
            .ok()
            .and_then(|value| value.parse().ok());
        if !cfg!(any(target_os = "windows", target_os = "linux", target_os = "android"))
            && override_count.is_none()
        {
            return Executor::SingleThread;
        }
        let available = std::thread::available_parallelism().map_or(1, usize::from);
        let threads = thread_count(available, cfg!(target_os = "android"), override_count);
        if threads == 1 {
            return Executor::SingleThread;
        }
        match rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("background-tensor-{i}"))
            .build()
        {
            Ok(pool) => {
                log::info!(target: "schist_neural::execution", "background tensors: {threads} CPU workers");
                Executor::MultiThread(Arc::new(pool))
            }
            Err(error) => {
                log::warn!("background tensor workers unavailable: {error}");
                Executor::SingleThread
            }
        }
    })
}

fn scoped<T>(executor: Executor, run: impl FnOnce() -> T) -> T {
    // tract's TLS scope restores only on normal return. Catch inside it, then
    // resume outside so a caught application panic cannot leak this executor
    // into the next job on the editor's background worker.
    let result = multithread_tract_scope(executor, || {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
    });
    match result {
        Ok(value) => value,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

pub(super) fn run<T>(id: &str, run: impl FnOnce() -> T) -> T {
    if !matches!(
        id,
        "foreground" | "foreground-matting" | "detail-matting" | "subject-guide" | "matting"
    ) {
        return run();
    }
    scoped(executor().clone(), run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tract_linalg::multithread::{current_tract_executor, par_chunks_mut};

    #[test]
    fn worker_count_respects_mobile_and_available_cpu_limits() {
        assert_eq!(thread_count(16, false, None), 4);
        assert_eq!(thread_count(8, true, None), 2);
        assert_eq!(thread_count(1, false, None), 1);
        assert_eq!(thread_count(0, true, None), 1);
        assert_eq!(thread_count(3, false, Some(8)), 3);
        assert_eq!(thread_count(16, false, Some(99)), 8);
        assert_eq!(thread_count(16, true, Some(0)), 1);
    }

    #[test]
    fn scoped_workers_restore_after_nesting_errors_and_panics() {
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        scoped(Executor::MultiThread(pool.clone()), || {
            let current_is_outer = || matches!(current_tract_executor(), Executor::MultiThread(ref p) if Arc::ptr_eq(p, &pool));
            assert!(current_is_outer());
            assert_eq!(scoped(Executor::SingleThread, || Err::<(), _>(7)), Err(7));
            assert!(current_is_outer());
            assert!(
                std::panic::catch_unwind(|| scoped(Executor::SingleThread, || panic!(
                    "test unwind"
                )))
                .is_err()
            );
            assert!(current_is_outer());
            run("detail", || assert!(current_is_outer()));
        });
    }

    #[test]
    fn threaded_rows_retain_order_and_do_real_work_on_the_pool() {
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let mut output = vec![0u64; 128 * 1024];
        scoped(Executor::MultiThread(pool), || {
            let len = output.len();
            par_chunks_mut(&mut output, 128, len, |first_row, chunk| {
                assert!(rayon::current_thread_index().is_some());
                for (i, value) in chunk.iter_mut().enumerate() {
                    *value = (first_row * 128 + i) as u64;
                }
                Ok(())
            })
            .unwrap();
        });
        assert!(output.iter().enumerate().all(|(i, &v)| v == i as u64));
    }
}
