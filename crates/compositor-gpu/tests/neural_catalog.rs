#![cfg(not(target_arch = "wasm32"))]

use schist_compositor_gpu::GpuContext;
use schist_fx::{ComputeJob, FxBackend};
use schist_neural::{Fit, Input, Model, ModelSource, ModelSpec, Range};
use std::sync::{Arc, Mutex};
static BACKEND: Mutex<()> = Mutex::new(());

struct Tracking {
    ctx: GpuContext,
    seen: Mutex<Vec<&'static str>>,
    decline_after: usize,
    minimum_work: usize,
}
impl FxBackend for Tracking {
    fn name(&self) -> &'static str {
        "neural parity"
    }
    fn compute_available(&self, work: usize) -> bool {
        work >= self.minimum_work
    }
    fn compute(&self, job: &ComputeJob<'_>) -> Option<Vec<f32>> {
        let mut seen = self.seen.lock().unwrap();
        if seen.len() >= self.decline_after || job.program.work < self.minimum_work {
            return None;
        }
        let result = self
            .ctx
            .run_compute(job)
            .expect("neural GPU dispatch declined");
        seen.extend(job.program.steps.iter().map(|s| s.shader.name));
        Some(result)
    }
}
struct Restore(Arc<dyn FxBackend>);
impl Drop for Restore {
    fn drop(&mut self) {
        schist_fx::set_backend(self.0.clone());
    }
}
fn spec(input: Input) -> &'static ModelSpec {
    Box::leak(Box::new(ModelSpec {
        id: "gpu-fixture",
        name: "GPU fixture",
        file: "",
        source: ModelSource::BuiltIn,
        sha256: None,
        bytes: 0,
        input,
        range: Range::Unit,
        license: "",
        note: "",
    }))
}
fn frame(width: usize, height: usize) -> Input {
    Input::Frame {
        width,
        height,
        fit: Fit::Stretch,
    }
}
fn close(a: &[f32], b: &[f32], tolerance: f32) {
    assert_eq!(a.len(), b.len());
    for (i, (&a, &b)) in a.iter().zip(b).enumerate() {
        assert!(
            a.is_finite() && (a - b).abs() <= tolerance * (1.0 + b.abs()),
            "at {i}: GPU={a}, CPU={b}"
        );
    }
}

// One test owns the global backend throughout, so these checks cannot race.
#[test]
fn missing_catalogue_operations_execute_and_fall_back_correctly() {
    let _lock = BACKEND.lock().unwrap_or_else(|p| p.into_inner());
    let _restore = Restore(schist_fx::backend());
    let gpu = Arc::new(Tracking {
        ctx: GpuContext::new().expect("GPU parity requires an adapter"),
        seen: Mutex::new(vec![]),
        decline_after: usize::MAX,
        minimum_work: 0,
    });
    eprintln!("neural parity adapter: {:?}", gpu.ctx.adapter_info());
    for (bytes, input, resident) in [
        (
            include_bytes!("../../neural/tests/fixtures/gpu-prelu-dropout.onnx").as_slice(),
            frame(5, 4),
            true,
        ),
        (
            include_bytes!("../../neural/tests/fixtures/gpu-partitioned-conv.onnx").as_slice(),
            frame(27, 31),
            false,
        ),
        (
            include_bytes!("../../neural/tests/fixtures/gpu-partitioned-einsum.onnx").as_slice(),
            frame(5, 4),
            false,
        ),
        (
            include_bytes!("../../neural/tests/fixtures/gpu-partitioned-bands.onnx").as_slice(),
            frame(1027, 684),
            false,
        ),
        (
            include_bytes!("../../neural/tests/fixtures/gpu-partitioned-tokens.onnx").as_slice(),
            Input::Tokens { context: 7 },
            false,
        ),
    ] {
        let model = Model::from_bytes(spec(input), bytes).unwrap();
        assert_eq!(model.gpu_program().is_some(), resident);
        if !resident {
            assert!(model.gpu_partition_count() > 0);
        }
        let (w, h) = input.dims();
        let rgb = (0..w * h * 3)
            .map(|i| (i * 197 % 1009) as f32 / 503.0 - 1.0)
            .collect::<Vec<_>>();
        let run = || {
            if matches!(input, Input::Tokens { .. }) {
                model.run_token_scores(&[1, 16, 4, 0, 7, 3, 2]).unwrap()
            } else {
                model.run_scores(&rgb).unwrap()
            }
        };
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        let expected = run();
        gpu.seen.lock().unwrap().clear();
        schist_fx::set_backend(gpu.clone());
        let actual = run();
        close(&actual, &expected, 3e-5);
        assert!(
            !gpu.seen.lock().unwrap().is_empty(),
            "model silently ran on CPU"
        );
        if w == 1027 {
            assert!(
                gpu.seen.lock().unwrap().len() > 1,
                "large convolution was not banded"
            );
        }
        let failing = Arc::new(Tracking {
            ctx: GpuContext::new().unwrap(),
            seen: Mutex::new(vec![]),
            decline_after: usize::from(w == 1027),
            minimum_work: 0,
        });
        schist_fx::set_backend(failing.clone());
        close(&run(), &expected, 3e-5);
        if w == 1027 {
            assert_eq!(failing.seen.lock().unwrap().len(), 1);
            let thresholded = Arc::new(Tracking {
                ctx: GpuContext::new().unwrap(),
                seen: Mutex::new(vec![]),
                decline_after: usize::MAX,
                minimum_work: 8_000_000,
            });
            schist_fx::set_backend(thresholded.clone());
            close(&run(), &expected, 3e-5);
            assert!(
                thresholded.seen.lock().unwrap().len() > 1,
                "short final band incorrectly triggered the CPU threshold"
            );
        }
    }
}

#[test]
fn every_installed_catalogue_model_has_a_gpu_path() {
    let _lock = BACKEND.lock().unwrap_or_else(|p| p.into_inner());
    for spec in schist_neural::CATALOG {
        if !spec.built_in() && !schist_neural::model_dir().join(spec.file).is_file() {
            continue;
        }
        let model =
            schist_neural::get(spec.id).unwrap_or_else(|| panic!("{} failed to load", spec.id));
        assert!(
            model.gpu_program().is_some() || model.gpu_partition_count() > 0,
            "{} has no GPU path",
            spec.id
        );
        schist_neural::release(spec.id);
    }
}

/// Use the actual downloadable weights, without making routine CI fetch them.
#[test]
fn downloaded_catalogue_models_match_cpu() {
    let Some(directory) = std::env::var_os("SCHIST_GPU_MODEL_DIR") else {
        return;
    };
    let _lock = BACKEND.lock().unwrap_or_else(|p| p.into_inner());
    let _restore = Restore(schist_fx::backend());
    let gpu = Arc::new(Tracking {
        ctx: GpuContext::new().unwrap(),
        seen: Mutex::new(vec![]),
        decline_after: usize::MAX,
        minimum_work: 0,
    });
    for spec in schist_neural::CATALOG.iter().filter(|s| !s.built_in()) {
        let id = spec.id;
        eprintln!("Checking {id} GPU execution");
        let bytes = std::fs::read(std::path::Path::new(&directory).join(spec.file)).unwrap();
        let model = Model::from_bytes(spec, &bytes).unwrap();
        let (w, h) = spec.input.dims();
        let rgb = (0..w * h * 3)
            .map(|i| (i * 11 % 1009) as f32 / 1008.0)
            .collect::<Vec<_>>();
        let tokens = (0..w)
            .map(|i| {
                if i == 0 {
                    49406
                } else if i == 7 {
                    49407
                } else {
                    (i % 7) as i64
                }
            })
            .collect::<Vec<_>>();
        let run = || match spec.input {
            Input::Tokens { .. } => model.run_token_scores(&tokens).unwrap(),
            // Compare image models in their public, decoded RGB range. The
            // style graphs internally produce byte-range floats; comparing
            // those to a unit-range image tolerance overstates their error.
            Input::Tiles { .. } => model.run_tile(&rgb).unwrap(),
            Input::Frame { .. } => model.run_scores(&rgb).unwrap(),
        };
        schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
        let expected = run();
        gpu.seen.lock().unwrap().clear();
        schist_fx::set_backend(gpu.clone());
        let actual = run();
        let maximum = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        eprintln!("{id}: maximum absolute error {maximum}");
        close(&actual, &expected, 3e-4);
        assert!(!gpu.seen.lock().unwrap().is_empty(), "{id} did not offload");
        eprintln!("{id}: {} GPU dispatches", gpu.seen.lock().unwrap().len());
    }
}

/// Full-network parity for the bundled weights at a smaller test resolution.
/// The shipping 2048px contract is checked separately; the
/// large fixture above exercises bands and a failure after the first band.
#[test]
fn anti_smudge_network_matches_cpu() {
    let _lock = BACKEND.lock().unwrap_or_else(|p| p.into_inner());
    let _restore = Restore(schist_fx::backend());
    let bytes = include_bytes!("../../neural/models/anti-smudge.onnx.xz");
    let mut spec = schist_neural::spec("anti-smudge").unwrap().clone();
    // A fixture ID prevents the shipping restoration metadata from overriding
    // the small input fact. These are the exact compressed shipping weights.
    spec.id = "anti-smudge-gpu-parity";
    spec.input = Input::Tiles {
        size: 128,
        overlap: 16,
        scale: 1,
    };
    let model = Model::from_bytes(Box::leak(Box::new(spec)), bytes).unwrap();
    let rgb = (0..128 * 128 * 3)
        .map(|i| (i * 11 % 1009) as f32 / 1008.0)
        .collect::<Vec<_>>();
    schist_fx::set_backend(Arc::new(schist_fx::CpuFx));
    let expected = model.run_tile(&rgb).unwrap();
    let gpu = Arc::new(Tracking {
        ctx: GpuContext::new().unwrap(),
        seen: Mutex::new(vec![]),
        decline_after: usize::MAX,
        minimum_work: 0,
    });
    schist_fx::set_backend(gpu.clone());
    let actual = model.run_tile(&rgb).unwrap();
    close(&actual, &expected, 3e-4);
    let seen = gpu.seen.lock().unwrap();
    assert!(seen.contains(&"neural-convolution-band"));
    assert!(seen.contains(&"neural-contraction"));
    eprintln!("Anti-Smudge: {} GPU dispatches", seen.len());
}
