//! End-to-end native inference, also useful for comparing tract with Python ORT.
//! make background-removal-example ARGS='input.png output.png [runs] [--reload] [--preload]'

use anyhow::{bail, Context, Result};
use std::time::Instant;

struct TimingLog;
impl log::Log for TimingLog {
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

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(2..=5).contains(&args.len()) {
        bail!("usage: remove_background input output.png [runs] [--reload] [--preload]");
    }
    log::set_logger(&TimingLog).unwrap();
    log::set_max_level(log::LevelFilter::Info);
    let mut runs = 1;
    let mut reload = false;
    let mut preload = false;
    for option in &args[2..] {
        match option.to_str() {
            Some("--reload") => reload = true,
            Some("--preload") => preload = true,
            Some(value) => runs = value.parse::<usize>()?,
            None => bail!("invalid option"),
        }
    }
    if !(1..=10).contains(&runs) || (preload && reload) {
        bail!("invalid benchmark options");
    }
    let output = std::path::Path::new(&args[1]);
    if output.exists() {
        bail!("output already exists");
    }
    let source = image::open(&args[0])?.to_rgba8();
    let (w, h) = (source.width() as usize, source.height() as usize);
    if w.checked_mul(h).is_none_or(|n| n == 0 || n > 16_777_216) {
        bail!("invalid or oversized image");
    }
    let rgb: Vec<f32> = source
        .pixels()
        .flat_map(|p| {
            let a = p[3] as f32 / 255.0;
            [p[0], p[1], p[2]].map(|v| v as f32 / 255.0 * a + 0.5 * (1.0 - a))
        })
        .collect();
    let load = |id: &str| {
        if reload {
            schist_neural::forget(id);
        }
        let start = Instant::now();
        let model = schist_neural::get(id);
        if reload {
            schist_neural::forget(id);
        } else {
            schist_neural::release(id);
        }
        eprintln!("{id} load: {:.3}s", start.elapsed().as_secs_f64());
        model
    };
    if preload {
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "linux",
            target_os = "windows"
        ))]
        {
            let start = Instant::now();
            if let Some(thread) = schist_neural::preload_background_removal() {
                thread.join().unwrap();
            }
            eprintln!("startup preparation: {:.3}s", start.elapsed().as_secs_f64());
        }
    }
    for run in 1..=runs {
        eprintln!("run {run}/{runs}");
        if reload {
            // The opaque-core refiner loads this small model internally.
            schist_neural::forget("matting");
        }
        let pipeline = || -> Result<()> {
            let now = Instant::now();
            let detector_id = schist_neural::foreground_model_id();
            let foreground = load(detector_id).context("install the foreground model first")?;
            let start = Instant::now();
            let raw_coarse = schist_neural::foreground(&foreground, &rgb, w, h)?;
            eprintln!(
                "{detector_id} inference: {:.3}s",
                start.elapsed().as_secs_f64()
            );
            drop(foreground);
            let reference = if detector_id == "foreground-matting" {
                let general = load("foreground").context("general detector unavailable")?;
                let start = Instant::now();
                let alpha = schist_neural::foreground(&general, &rgb, w, h)?;
                eprintln!(
                    "foreground inference: {:.3}s",
                    start.elapsed().as_secs_f64()
                );
                Some(alpha)
            } else {
                None
            };
            let guide = load("subject-guide").context("subject guide unavailable")?;
            let start = Instant::now();
            let coarse = schist_neural::guide_foreground_with_reference(
                &guide,
                &rgb,
                &raw_coarse,
                reference.as_deref(),
                w,
                h,
            )?;
            eprintln!(
                "subject-guide inference: {:.3}s",
                start.elapsed().as_secs_f64()
            );
            drop(guide);
            drop(reference);
            drop(raw_coarse);
            let refiner_id = schist_neural::matting_model_id();
            let model = load(refiner_id).context("matting model unavailable")?;
            let start = Instant::now();
            let alpha = schist_neural::refine_alpha(&model, &rgb, &coarse, w, h)?;
            eprintln!(
                "{refiner_id} inference: {:.3}s",
                start.elapsed().as_secs_f64()
            );
            drop(model);
            drop(coarse);
            let start = Instant::now();
            let colors = schist_neural::clean_foreground(&rgb, &alpha, w, h)?;
            eprintln!("color cleanup: {:.3}s", start.elapsed().as_secs_f64());
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
            eprintln!("processing: {:.3}s", now.elapsed().as_secs_f64());
            if run == runs {
                result.save_with_format(output, image::ImageFormat::Png)?;
            }
            eprintln!("{}x{} in {:.2}s", w, h, now.elapsed().as_secs_f32());
            Ok(())
        };
        #[cfg(not(target_arch = "wasm32"))]
        schist_neural::with_adaptive_execution(pipeline)?;
        #[cfg(target_arch = "wasm32")]
        pipeline()?;
    }
    Ok(())
}
