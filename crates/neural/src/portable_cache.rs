//! Retain the fixed desktop background pipeline briefly between layer actions.
//! Phone/Web workers still release plans at every stage. Track weak loading
//! slots, so invalidation never resurrects a model or keeps an old plan alive.
use crate::model_cache::Cache;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

const IDLE_TIME: Duration = Duration::from_secs(300);
type Slot<T> = OnceLock<Option<Arc<T>>>;
type Idle<T> = HashMap<String, (Weak<Slot<T>>, Instant)>;

fn background(id: &str) -> bool {
    matches!(
        id,
        "foreground" | "foreground-matting" | "subject-guide" | "matting" | "detail-matting"
    )
}

fn retain<T>(cache: &Cache<T>, idle: &Mutex<Idle<T>>, id: &str, now: Instant) {
    let Ok(mut cache) = cache.write() else { return };
    let Ok(mut idle) = idle.lock() else { return };
    for key in [id.to_owned(), format!("center:{id}")] {
        if let Some(slot) = cache.get(&key) {
            if slot.get().is_some_and(Option::is_some) {
                idle.insert(key, (Arc::downgrade(slot), now));
            } else {
                // Retain only successfully prepared models, not failed loads.
                cache.remove(&key);
                idle.remove(&key);
            }
        }
    }
}

fn prune<T>(cache: &Cache<T>, idle: &Mutex<Idle<T>>, now: Instant) {
    let mut expired = Vec::new();
    if let (Ok(mut cache), Ok(mut idle)) = (cache.write(), idle.lock()) {
        idle.retain(|key, (previous, used)| {
            let Some(slot) = cache.get(key) else {
                return false;
            };
            // A new installation can use the same key. Never expire its slot
            // using an older generation's deadline, even during a slow load.
            if !Weak::ptr_eq(previous, &Arc::downgrade(slot)) {
                return false;
            }
            if now.saturating_duration_since(*used) >= IDLE_TIME
                && Arc::strong_count(slot) == 1
                && slot.get().is_some_and(|model| {
                    model
                        .as_ref()
                        .is_none_or(|model| Arc::strong_count(model) == 1)
                })
            {
                expired.extend(cache.remove(key));
                false
            } else {
                true
            }
        });
    }
    // Destroying optimized plans can be slow. Never block other model loads on
    // their destructors, or run them while holding either bookkeeping lock.
    drop(expired);
}

/// True means this release is managed by the desktop idle pool. At most the
/// five background models (plus the full/cropped detail variant) are eligible.
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) fn release(id: &str) -> bool {
    static STARTED: OnceLock<bool> = OnceLock::new();
    static IDLE: OnceLock<Mutex<Idle<crate::Model>>> = OnceLock::new();
    if !background(id) {
        return false;
    }
    let idle = IDLE.get_or_init(Default::default);
    if !*STARTED.get_or_init(|| {
        match std::thread::Builder::new()
            .name("background-model-idle".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(60));
                prune(crate::cache(), idle, Instant::now());
            }) {
            Ok(_) => true,
            Err(error) => {
                log::warn!("background model idle thread: {error}");
                false
            }
        }
    }) {
        return false;
    }
    retain(crate::cache(), idle, id, Instant::now());
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_cache::get;

    #[test]
    fn only_the_bounded_background_pipeline_is_retained() {
        for id in [
            "foreground",
            "foreground-matting",
            "subject-guide",
            "matting",
            "detail-matting",
        ] {
            assert!(background(id));
        }
        assert!(!background("face-embed"));
        assert!(!background("arbitrary-download"));
    }

    #[test]
    fn consecutive_actions_reuse_plans_until_idle_without_evicting_active_users() {
        let cache = Cache::default();
        let idle = Mutex::new(Idle::default());
        let start = Instant::now();
        let first = get(&cache, "foreground".into(), || Some(7)).unwrap();
        retain(&cache, &idle, "foreground", start);
        let next = get(&cache, "foreground".into(), || {
            panic!("plan must be reused")
        })
        .unwrap();
        assert!(Arc::ptr_eq(&first, &next));
        let weak = Arc::downgrade(&first);
        let later = start + IDLE_TIME + Duration::from_secs(1);
        prune(&cache, &idle, later);
        assert!(!cache.read().unwrap().is_empty());
        drop(first);
        drop(next);
        // The slot-to-model handoff is also an active user.
        let slot = cache.read().unwrap()["foreground"].clone();
        prune(&cache, &idle, later);
        assert!(weak.upgrade().is_some());
        drop(slot);
        retain(&cache, &idle, "foreground", later);
        prune(&cache, &idle, later);
        assert!(weak.upgrade().is_some());
        prune(&cache, &idle, later + IDLE_TIME);
        assert!(weak.upgrade().is_none());
        assert!(idle.lock().unwrap().is_empty());
    }

    #[test]
    fn cropped_plans_expire_but_invalidation_and_failed_loads_are_not_retained() {
        let cache = Cache::default();
        let idle = Mutex::new(Idle::default());
        let start = Instant::now();
        get(&cache, "center:detail-matting".into(), || Some(1)).unwrap();
        retain(&cache, &idle, "detail-matting", start);
        let old = Arc::downgrade(&cache.read().unwrap()["center:detail-matting"]);
        cache.write().unwrap().remove("center:detail-matting");
        assert!(old.upgrade().is_none());
        get(&cache, "center:detail-matting".into(), || Some(2)).unwrap();
        prune(&cache, &idle, start + IDLE_TIME);
        assert!(cache.read().unwrap().contains_key("center:detail-matting"));
        retain(&cache, &idle, "detail-matting", start + IDLE_TIME);
        prune(&cache, &idle, start + IDLE_TIME * 2);
        assert!(cache.read().unwrap().is_empty());
        assert!(get(&cache, "foreground".into(), || None).is_none());
        retain(&cache, &idle, "foreground", start);
        assert!(cache.read().unwrap().is_empty());
        assert!(idle.lock().unwrap().is_empty());
    }
}
