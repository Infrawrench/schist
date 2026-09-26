//! Compare a compiled detail-refinement candidate through the actual Rust pipeline.
//! make profile-background-coreml ARGS='input.jpg output.png 3 [model.mlmodelc|--preload]'
#[cfg(all(target_os = "macos", feature = "coreml-export"))]
fn main() -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    use schist_neural as neural;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Instant;

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        (3..=4).contains(&args.len()),
        "usage: profile_coreml input output.png runs [model.mlmodelc|--preload]"
    );
    let runs: usize = args[2].to_str().context("invalid runs")?.parse()?;
    ensure!((1..=10).contains(&runs), "runs must be 1..10");
    ensure!(!Path::new(&args[1]).exists(), "output exists");
    let source = image::open(&args[0])?.to_rgba8();
    let (w, h) = (source.width() as usize, source.height() as usize);
    ensure!(w * h <= 16_777_216, "input too large");
    let rgb: Vec<f32> = source
        .pixels()
        .flat_map(|p| {
            let a = p[3] as f32 / 255.0;
            [p[0], p[1], p[2]].map(|v| v as f32 / 255.0 * a + 0.5 * (1.0 - a))
        })
        .collect();
    if args.get(3).is_some_and(|arg| arg == "--preload") {
        let start = Instant::now();
        if let Some(worker) = neural::preload_background_removal() {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("preload panicked"))?;
        }
        eprintln!("startup preload: {:.3}s", start.elapsed().as_secs_f64());
    }
    neural::with_adaptive_execution(|| -> anyhow::Result<()> {
        let mut candidate = None;
        for run in 1..=runs {
            eprintln!("run {run}/{runs}");
            let total = Instant::now();
            let report = |name: &str, start: Instant| {
                eprintln!("{name}: {:.3}s", start.elapsed().as_secs_f64());
            };
            let load = |id: &str| -> anyhow::Result<_> {
                let start = Instant::now();
                let model = neural::get(id).with_context(|| format!("model unavailable: {id}"))?;
                neural::release(id);
                report(&format!("{id} load"), start);
                Ok(model)
            };
            let id = neural::foreground_model_id();
            let model = load(id)?;
            let start = Instant::now();
            let raw = neural::foreground(&model, &rgb, w, h)?;
            report(id, start);
            drop(model);
            let reference = if id == "foreground-matting" {
                let model = load("foreground")?;
                let start = Instant::now();
                let reference = neural::foreground(&model, &rgb, w, h)?;
                report("foreground", start);
                Some(reference)
            } else {
                None
            };
            let model = load("subject-guide")?;
            let start = Instant::now();
            let coarse = neural::guide_foreground_with_reference(
                &model,
                &rgb,
                &raw,
                reference.as_deref(),
                w,
                h,
            )?;
            report("subject-guide", start);
            drop(model);
            drop(raw);
            drop(reference);
            let model = if let Some(path) = args.get(3).filter(|arg| *arg != "--preload") {
                if candidate.is_none() {
                    let start = Instant::now();
                    candidate = Some(Arc::new(neural::Model::from_coreml_path(
                        neural::spec("detail-matting").context("missing detail model")?,
                        Path::new(path),
                    )?));
                    report("candidate load", start);
                }
                candidate.as_ref().unwrap().clone()
            } else {
                load("detail-matting")?
            };
            let start = Instant::now();
            let alpha = neural::refine_alpha(&model, &rgb, &coarse, w, h)?;
            report("detail-matting", start);
            drop(model);
            drop(coarse);
            let start = Instant::now();
            let colors = neural::clean_foreground(&rgb, &alpha, w, h)?;
            report("color cleanup", start);
            let mut result = source.clone();
            for ((pixel, alpha), colors) in result
                .pixels_mut()
                .zip(&alpha)
                .zip(colors.as_chunks::<3>().0)
            {
                if pixel[3] == 255 && *alpha > 0.0 && *alpha < 1.0 {
                    for c in 0..3 {
                        pixel[c] = (colors[c] * 255.0).round() as u8;
                    }
                }
                pixel[3] = (pixel[3] as f32 * alpha).round() as u8;
            }
            report("pipeline", total);
            if run == runs {
                result.save_with_format(&args[1], image::ImageFormat::Png)?;
            }
            report("total", total);
        }
        Ok(())
    })
}

#[cfg(not(all(target_os = "macos", feature = "coreml-export")))]
fn main() {
    eprintln!("Use make profile-background-coreml on macOS.");
    std::process::exit(1);
}
