//! Executed inside an iOS app, with the same public pipeline as the editor.
use super::*;

#[test]
#[ignore = "install foreground weights in the test container; see docs/ios.md"]
fn ios_background_pipeline_uses_native_models_and_releases_each_stage() {
    with_adaptive_execution(|| {
        let (rgb, w, h) = if let Some(path) = std::env::var_os("SCHIST_IOS_TEST_IMAGE") {
            let image = image::open(path).unwrap().to_rgb8();
            let (w, h) = (image.width() as usize, image.height() as usize);
            (
                image
                    .into_raw()
                    .into_iter()
                    .map(|v| v as f32 / 255.)
                    .collect(),
                w,
                h,
            )
        } else {
            let rgb = include_bytes!("../tests/fixtures/photo.rgb")
                .iter()
                .map(|&v| v as f32 / 255.)
                .collect::<Vec<_>>();
            (rgb, 128, 128)
        };
        let start = std::time::Instant::now();
        let detector = get("foreground").expect("install foreground weights in SCHIST_MODEL_DIR");
        #[cfg(target_arch = "aarch64")]
        assert!(detector.native.is_some(), "detector fell back to tract");
        release("foreground");
        assert_eq!(Arc::strong_count(&detector), 1);
        let weak = Arc::downgrade(&detector);
        let coarse = foreground(&detector, &rgb, w, h).unwrap();
        #[cfg(target_arch = "aarch64")]
        assert!(
            detector.uses_native_inference(),
            "prediction fell back to tract"
        );
        drop(detector);
        assert!(weak.upgrade().is_none());
        let guide = get("subject-guide").unwrap();
        release("subject-guide");
        assert!(guide.uses_native_inference());
        let coarse = guide_foreground(&guide, &rgb, &coarse, w, h).unwrap();
        drop(guide);
        let refiner = get("detail-matting").unwrap();
        release("detail-matting");
        let alpha = refine_alpha(&refiner, &rgb, &coarse, w, h).unwrap();
        drop(refiner);
        let colors = clean_foreground(&rgb, &alpha, w, h).unwrap();
        assert_eq!(alpha.len(), w * h);
        assert!(alpha
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        assert_eq!(colors.len(), rgb.len());
        assert!(colors
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        let cache = cache().read().unwrap();
        for id in ["foreground", "subject-guide", "matting", "detail-matting"] {
            assert!(!cache.contains_key(id));
            assert!(!cache.contains_key(&format!("native:{id}")));
            assert!(!cache.contains_key(&format!("compiled:gpu:{id}")));
            assert!(!cache.contains_key(&format!("compiled:cpu:{id}")));
        }
        eprintln!(
            "iOS background pipeline {w}x{h}: {:.3}s",
            start.elapsed().as_secs_f64()
        );
    });
}

#[test]
#[ignore = "iOS inference scheduling; make check-background-removal-ios-native"]
fn ios_background_serializes_workers_and_allows_nested_execution() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let active = AtomicUsize::new(0);
    let barrier = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                barrier.wait();
                with_adaptive_execution(|| {
                    assert_eq!(active.fetch_add(1, Ordering::SeqCst), 0);
                    with_adaptive_execution(std::thread::yield_now);
                    assert_eq!(active.fetch_sub(1, Ordering::SeqCst), 1);
                });
            });
        }
    });
    let panic = std::panic::catch_unwind(|| with_adaptive_execution(|| panic!("cancelled")));
    assert!(panic.is_err());
    with_adaptive_execution(|| assert!(crate::execution::adaptive_enabled()));
    assert!(!crate::execution::adaptive_enabled());
}
