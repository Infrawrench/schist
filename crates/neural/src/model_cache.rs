//! Publish the loading slot before opening a model. Callers share one load,
//! while other model keys and cache invalidation remain independent.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

pub(super) type Cache<T> = RwLock<HashMap<String, Arc<OnceLock<Option<Arc<T>>>>>>;

#[cfg(any(not(schist_library), test))]
pub(super) fn get<T>(
    cache: &Cache<T>,
    key: String,
    load: impl FnOnce() -> Option<T>,
) -> Option<Arc<T>> {
    let existing = cache.read().ok()?.get(&key).cloned();
    let slot = match existing {
        Some(slot) => slot,
        None => cache.write().ok()?.entry(key).or_default().clone(),
    };
    // Never hold the map lock while loading, warming, or waiting. Removing a
    // slot during a load prevents that old result from repopulating the cache.
    slot.get_or_init(|| load().map(Arc::new)).clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Barrier,
    };

    #[test]
    fn concurrent_requests_share_one_load_and_one_result() {
        let cache = Cache::default();
        let calls = AtomicUsize::new(0);
        let barrier = Barrier::new(8);
        std::thread::scope(|scope| {
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        get(&cache, "model".into(), || {
                            calls.fetch_add(1, Ordering::Relaxed);
                            Some(42)
                        })
                        .unwrap()
                    })
                })
                .collect();
            let models: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
            assert!(models.iter().all(|m| Arc::ptr_eq(m, &models[0])));
        });
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn invalidation_during_loading_cannot_restore_stale_result_or_block_other_keys() {
        let cache = Cache::default();
        let (started_tx, started_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let cache_ref = &cache;
            let worker = scope.spawn(move || {
                get(cache_ref, "model".into(), || {
                    started_tx.send(()).unwrap();
                    finish_rx.recv().unwrap();
                    Some(1)
                })
            });
            started_rx.recv().unwrap();
            assert_eq!(*get(&cache, "other".into(), || Some(7)).unwrap(), 7);
            cache.write().unwrap().remove("model");
            assert_eq!(*get(&cache, "model".into(), || Some(2)).unwrap(), 2);
            finish_tx.send(()).unwrap();
            assert_eq!(*worker.join().unwrap().unwrap(), 1);
        });
        assert_eq!(
            *get(&cache, "model".into(), || panic!("already loaded")).unwrap(),
            2
        );
    }

    #[test]
    fn failures_are_shared_until_invalidated() {
        let cache = Cache::<u8>::default();
        assert!(get(&cache, "broken".into(), || None).is_none());
        assert!(get(&cache, "broken".into(), || panic!("cached failure")).is_none());
        cache.write().unwrap().remove("broken");
        assert_eq!(*get(&cache, "broken".into(), || Some(3)).unwrap(), 3);
    }
}
