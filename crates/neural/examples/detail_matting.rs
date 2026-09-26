//! Check native detail matting independently of the slow detector passes.
//! make detail-matting-example ARGS='source.png coarse.f32 output.png'
use anyhow::{bail, Context, Result};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        bail!("usage: detail_matting image coarse.f32 output.png");
    }
    if std::path::Path::new(&args[2]).exists() {
        bail!("output already exists");
    }
    let mut image = image::open(&args[0])?.to_rgba8();
    let (w, h) = (image.width() as usize, image.height() as usize);
    let bytes = std::fs::read(&args[1])?;
    if bytes.len() != w * h * 4 {
        bail!("coarse buffer length differs");
    }
    let coarse: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    let rgb: Vec<f32> = image
        .pixels()
        .flat_map(|p| {
            let a = p[3] as f32 / 255.0;
            [p[0], p[1], p[2]].map(|v| v as f32 / 255.0 * a + 0.5 * (1.0 - a))
        })
        .collect();
    let now = std::time::Instant::now();
    let model = schist_neural::get("detail-matting").context("detail matting model unavailable")?;
    schist_neural::release("detail-matting");
    eprintln!("model loaded in {:.2}s", now.elapsed().as_secs_f32());
    let alpha = schist_neural::refine_alpha(&model, &rgb, &coarse, w, h)?;
    drop(model);
    let colors = schist_neural::clean_foreground(&rgb, &alpha, w, h)?;
    for ((pixel, a), c) in image.pixels_mut().zip(alpha).zip(colors.as_chunks::<3>().0) {
        if pixel[3] == 255 && a > 0.0 && a < 1.0 {
            for channel in 0..3 {
                pixel[channel] = (c[channel] * 255.0).round() as u8;
            }
        }
        pixel[3] = (pixel[3] as f32 * a).round() as u8;
    }
    image.save_with_format(&args[2], image::ImageFormat::Png)?;
    eprintln!("{w}x{h} in {:.2}s", now.elapsed().as_secs_f32());
    Ok(())
}
