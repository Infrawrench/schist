//! Profile the actual background-removal pipeline, including model loading.
//! make profile-background-removal ARGS='auto input.jpg output.png 2'
use schist_compositor_gpu::GpuContext;
use schist_fx::{ComputeJob, FxBackend};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct PlacementLog;
impl log::Log for PlacementLog {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info && metadata.target() == "schist_neural::execution"
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{}", record.args());
        }
    }
    fn flush(&self) {}
}

struct Tracking {
    ctx: GpuContext,
    timing: Mutex<(usize, Duration)>,
    shaders: Mutex<BTreeMap<&'static str, (usize, Duration)>>,
}
impl FxBackend for Tracking {
    fn name(&self) -> &'static str {
        "background removal profiler"
    }
    fn compute_available(&self, work: usize) -> bool {
        schist_fx::worth_offloading(1, work)
    }
    fn compute(&self, job: &ComputeJob<'_>) -> Option<Vec<f32>> {
        if !self.compute_available(job.program.work) {
            return None;
        }
        let start = Instant::now();
        let result = self.ctx.run_compute(job).expect("GPU dispatch failed");
        let mut timing = self.timing.lock().unwrap();
        timing.0 += 1;
        timing.1 += start.elapsed();
        let mut shaders = self.shaders.lock().unwrap();
        let entry = shaders.entry(job.program.steps[0].shader.name).or_default();
        entry.0 += 1;
        entry.1 += start.elapsed();
        if timing.0.is_multiple_of(500) {
            eprintln!("GPU progress: {} submissions; {:?}", timing.0, *shaders);
        }
        Some(result)
    }
}

fn main() {
    log::set_logger(&PlacementLog).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    assert!(
        (3..=4).contains(&args.len()),
        "usage: background_removal cpu|gpu|auto input output.png [runs]"
    );
    assert!(args[0] == "cpu" || args[0] == "gpu" || args[0] == "auto");
    let runs = args
        .get(3)
        .map_or(1, |v| v.to_str().unwrap().parse::<usize>().unwrap());
    assert!((1..=10).contains(&runs));
    assert!(!std::path::Path::new(&args[2]).exists(), "output exists");
    let gpu = (args[0] != "cpu").then(|| {
        let ctx = GpuContext::new().expect("GPU adapter");
        eprintln!("adapter: {:?}", ctx.adapter_info());
        let gpu = Arc::new(Tracking {
            ctx,
            timing: Mutex::new((0, Duration::ZERO)),
            shaders: Mutex::new(BTreeMap::new()),
        });
        schist_fx::set_backend(gpu.clone());
        gpu
    });
    let source = image::open(&args[1]).unwrap().to_rgba8();
    let (w, h) = (source.width() as usize, source.height() as usize);
    assert!(w * h <= 16_777_216);
    let rgb: Vec<f32> = source
        .pixels()
        .flat_map(|p| {
            let a = p[3] as f32 / 255.0;
            [p[0], p[1], p[2]].map(|v| v as f32 / 255.0 * a + 0.5 * (1.0 - a))
        })
        .collect();
    for run in 1..=runs {
        eprintln!("run {run}/{runs}");
        let pipeline = || {
            let total = Instant::now();
            let report = |stage: &str, start: Instant| {
                let timing = gpu
                    .as_ref()
                    .map(|g| std::mem::take(&mut *g.timing.lock().unwrap()));
                eprintln!(
                    "{stage}: {:.3}s; GPU {timing:?}",
                    start.elapsed().as_secs_f64()
                );
                if let Some(gpu) = &gpu {
                    for (name, timing) in std::mem::take(&mut *gpu.shaders.lock().unwrap()) {
                        eprintln!("  {name}: {timing:?}");
                    }
                }
            };
            let load = |id: &str| {
                let start = Instant::now();
                let model = schist_neural::get(id).expect("model must be installed");
                schist_neural::release(id);
                eprintln!(
                    "{id}: resident={}, partitions={}",
                    model.gpu_program().is_some(),
                    model.gpu_partition_count()
                );
                report(&format!("{id} load"), start);
                model
            };
            let id = schist_neural::foreground_model_id();
            let model = load(id);
            let start = Instant::now();
            let raw = schist_neural::foreground(&model, &rgb, w, h).unwrap();
            report(id, start);
            drop(model);
            let reference = if id == "foreground-matting" {
                let model = load("foreground");
                let start = Instant::now();
                let alpha = schist_neural::foreground(&model, &rgb, w, h).unwrap();
                report("foreground", start);
                Some(alpha)
            } else {
                None
            };
            let model = load("subject-guide");
            let start = Instant::now();
            let coarse = schist_neural::guide_foreground_with_reference(
                &model,
                &rgb,
                &raw,
                reference.as_deref(),
                w,
                h,
            )
            .unwrap();
            report("subject-guide", start);
            drop(model);
            drop(raw);
            drop(reference);
            let model = load("detail-matting");
            let start = Instant::now();
            let alpha = schist_neural::refine_alpha(&model, &rgb, &coarse, w, h).unwrap();
            report("detail-matting", start);
            drop(model);
            drop(coarse);
            let start = Instant::now();
            let colors = schist_neural::clean_foreground(&rgb, &alpha, w, h).unwrap();
            report("color cleanup", start);
            let mut result = source.clone();
            for ((pixel, alpha), colors) in result
                .pixels_mut()
                .zip(alpha)
                .zip(colors.as_chunks::<3>().0)
            {
                if pixel[3] == 255 && alpha > 0.0 && alpha < 1.0 {
                    for c in 0..3 {
                        pixel[c] = (colors[c] * 255.0).round() as u8;
                    }
                }
                pixel[3] = (pixel[3] as f32 * alpha).round() as u8;
            }
            if run == runs {
                result
                    .save_with_format(&args[2], image::ImageFormat::Png)
                    .unwrap();
            }
            report("total", total);
        };
        if args[0] == "auto" {
            schist_neural::with_adaptive_execution(pipeline);
        } else {
            pipeline();
        }
    }
}
