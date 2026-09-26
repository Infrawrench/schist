//! End-to-end native inference, also useful for comparing tract with Python ORT.
//! make background-removal-example ARGS='input.png output.png'

use anyhow::{bail, Context, Result};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        bail!("usage: remove_background input output.png");
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
    let now = std::time::Instant::now();
    let detector_id = schist_neural::foreground_model_id();
    let foreground =
        schist_neural::get(detector_id).context("install the foreground model first")?;
    let raw_coarse = schist_neural::foreground(&foreground, &rgb, w, h)?;
    schist_neural::release(detector_id);
    drop(foreground);
    let reference = if detector_id == "foreground-matting" {
        let general = schist_neural::get("foreground").context("general detector unavailable")?;
        schist_neural::release("foreground");
        Some(schist_neural::foreground(&general, &rgb, w, h)?)
    } else {
        None
    };
    let guide = schist_neural::get("subject-guide").context("subject guide unavailable")?;
    let coarse = schist_neural::guide_foreground_with_reference(
        &guide,
        &rgb,
        &raw_coarse,
        reference.as_deref(),
        w,
        h,
    )?;
    schist_neural::release("subject-guide");
    drop(guide);
    drop(reference);
    drop(raw_coarse);
    let refiner_id = schist_neural::matting_model_id();
    let model = schist_neural::get(refiner_id).context("matting model unavailable")?;
    schist_neural::release(refiner_id);
    let alpha = schist_neural::refine_alpha(&model, &rgb, &coarse, w, h)?;
    drop(model);
    drop(coarse);
    let colors = schist_neural::clean_foreground(&rgb, &alpha, w, h)?;
    let mut result = source;
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
    result.save_with_format(output, image::ImageFormat::Png)?;
    eprintln!("{}x{} in {:.2}s", w, h, now.elapsed().as_secs_f32());
    Ok(())
}
