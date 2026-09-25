#![cfg(not(target_arch = "wasm32"))]

use schist_compositor_gpu::GpuContext;
use schist_fx::{ComputeJob, FxBackend};
use std::sync::{Arc, Mutex};

static BACKEND: Mutex<()> = Mutex::new(());
struct Tracking {
    ctx: GpuContext,
    dispatches: Mutex<usize>,
}
impl FxBackend for Tracking {
    fn name(&self) -> &'static str {
        "background matting parity"
    }
    fn compute_available(&self, work: usize) -> bool {
        work >= 8_000_000
    }
    fn compute(&self, job: &ComputeJob<'_>) -> Option<Vec<f32>> {
        if !self.compute_available(job.program.work) {
            return None;
        }
        let result = self
            .ctx
            .run_compute(job)
            .expect("matting GPU dispatch failed");
        *self.dispatches.lock().unwrap() += 1;
        Some(result)
    }
}
struct Restore(Arc<dyn FxBackend>);
impl Drop for Restore {
    fn drop(&mut self) {
        schist_fx::set_backend(self.0.clone());
    }
}

fn close(actual: &[f32], expected: &[f32], tolerance: f32) {
    assert_eq!(actual.len(), expected.len());
    let mut max = 0.0f32;
    for (a, b) in actual.iter().zip(expected) {
        assert!(a.is_finite());
        max = max.max((a - b).abs());
    }
    eprintln!("maximum alpha/probability difference: {max}");
    assert!(max <= tolerance, "GPU difference {max} exceeds {tolerance}");
}

fn read_alpha(path: impl AsRef<std::path::Path>) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap();
    let (values, remainder) = bytes.as_chunks::<4>();
    assert!(remainder.is_empty(), "truncated float32 alpha buffer");
    values.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

#[test]
fn bundled_refiners_and_guide_execute_on_gpu_and_match_cpu() {
    let _lock = BACKEND.lock().unwrap_or_else(|p| p.into_inner());
    let _restore = Restore(schist_fx::backend());
    let gpu = Arc::new(Tracking {
        ctx: GpuContext::new().expect("GPU adapter"),
        dispatches: Mutex::new(0),
    });
    eprintln!("background removal adapter: {:?}", gpu.ctx.adapter_info());
    for id in ["matting", "detail-matting", "subject-guide"] {
        let model = schist_neural::get(id).unwrap();
        schist_neural::release(id);
        assert!(model.gpu_program().is_some() || model.gpu_partition_count() > 0);
        let (w, h) = if id == "subject-guide" {
            (520, 520)
        } else {
            (73, 61)
        };
        let rgb = (0..w * h * 3)
            .map(|i| (i * 17 % 251) as f32 / 250.0)
            .collect::<Vec<_>>();
        let coarse = (0..w * h)
            .map(|i| ((i % w) as f32 / (w - 1) as f32).clamp(0.01, 0.99))
            .collect::<Vec<_>>();
        let run = || {
            if id == "subject-guide" {
                model.run_scores(&rgb).unwrap()
            } else {
                schist_neural::refine_alpha(&model, &rgb, &coarse, w, h).unwrap()
            }
        };
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        let expected = run();
        *gpu.dispatches.lock().unwrap() = 0;
        schist_fx::set_backend(gpu.clone());
        close(&run(), &expected, 5e-4);
        let count = *gpu.dispatches.lock().unwrap();
        assert!(count > 0, "{id} silently used CPU only");
        eprintln!("{id}: {count} GPU dispatches");
    }
}

/// Exercise the pinned detector weights when available locally, without
/// downloading hundreds of megabytes as part of routine CI.
#[test]
fn installed_background_detectors_execute_on_gpu_when_requested() {
    if std::env::var_os("SCHIST_BACKGROUND_GPU_DETECTORS").is_none() {
        return;
    }
    let _lock = BACKEND.lock().unwrap_or_else(|p| p.into_inner());
    let _restore = Restore(schist_fx::backend());
    let gpu = Arc::new(Tracking {
        ctx: GpuContext::new().expect("GPU adapter"),
        dispatches: Mutex::new(0),
    });
    let (rgb, w, h): (Vec<f32>, usize, usize) =
        if let Some(input) = std::env::var_os("SCHIST_MATTING_INPUT") {
            let image = image::open(input).unwrap().to_rgb8();
            let rgb = image.as_raw().iter().map(|v| *v as f32 / 255.0).collect();
            (rgb, image.width() as usize, image.height() as usize)
        } else {
            (
                (0..1024 * 1024 * 3)
                    .map(|i| (i * 17 % 251) as f32 / 250.0)
                    .collect(),
                1024,
                1024,
            )
        };
    for id in ["foreground", "foreground-matting"] {
        let model = schist_neural::get(id).unwrap_or_else(|| panic!("{id} not installed"));
        schist_neural::release(id);
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        let expected = schist_neural::foreground(&model, &rgb, w, h).unwrap();
        eprintln!(
            "{id}: CPU alpha range {}..{}; mean {}",
            expected.iter().copied().fold(f32::INFINITY, f32::min),
            expected.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            expected.iter().map(|a| *a as f64).sum::<f64>() / expected.len() as f64
        );
        if let Some(directory) = std::env::var_os("SCHIST_BACKGROUND_REFERENCE_DIR") {
            let reference = read_alpha(std::path::Path::new(&directory).join(format!("{id}.f32")));
            eprintln!("{id}: comparing independent runtime reference");
            close(&expected, &reference, 1.0 / 255.0);
        }
        *gpu.dispatches.lock().unwrap() = 0;
        schist_fx::set_backend(gpu.clone());
        let actual = schist_neural::foreground(&model, &rgb, w, h).unwrap();
        close(&actual, &expected, 1.0 / 255.0);
        let count = *gpu.dispatches.lock().unwrap();
        assert!(count > 0, "{id} silently used CPU only");
        eprintln!("{id}: {count} GPU dispatches");
    }
}

/// Optional private-image integration check. Never requires or commits photos.
#[test]
fn full_resolution_private_photo_matches_cpu_when_requested() {
    let Some(input) = std::env::var_os("SCHIST_MATTING_INPUT") else {
        return;
    };
    let coarse_path = std::env::var_os("SCHIST_MATTING_COARSE").expect("coarse float32 buffer");
    let _lock = BACKEND.lock().unwrap_or_else(|p| p.into_inner());
    let _restore = Restore(schist_fx::backend());
    let image = image::open(input).unwrap().to_rgb8();
    let (w, h) = (image.width() as usize, image.height() as usize);
    let rgb = image
        .as_raw()
        .iter()
        .map(|v| *v as f32 / 255.0)
        .collect::<Vec<_>>();
    let coarse = read_alpha(coarse_path);
    assert_eq!(coarse.len(), w * h);
    let model = schist_neural::get("detail-matting").unwrap();
    schist_neural::release("detail-matting");
    schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
    let expected = schist_neural::refine_alpha(&model, &rgb, &coarse, w, h).unwrap();
    let gpu = Arc::new(Tracking {
        ctx: GpuContext::new().unwrap(),
        dispatches: Mutex::new(0),
    });
    schist_fx::set_backend(gpu.clone());
    let actual = schist_neural::refine_alpha(&model, &rgb, &coarse, w, h).unwrap();
    close(&actual, &expected, 1.0 / 255.0);
    assert!(*gpu.dispatches.lock().unwrap() > 0);
    eprintln!("{w}x{h}: {} GPU dispatches", gpu.dispatches.lock().unwrap());
    if let Some(path) = std::env::var_os("SCHIST_MATTING_MASK_OUT") {
        let bytes = actual.iter().map(|a| (a * 255.0).round() as u8).collect();
        image::GrayImage::from_raw(w as u32, h as u32, bytes)
            .unwrap()
            .save(path)
            .unwrap();
    }
}
