//! Run exported Anti-Smudge weights through Schist's restoration runtime.
//! `make run-anti-smudge ARGS='model.onnx input.png output.png 0.6'`
use anyhow::{Context, Result};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(
        args.len() == 3 || args.len() == 4,
        "usage: anti_smudge model.onnx input.png output.png [strength 0..1]"
    );
    let strength: f32 = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(0.6);
    anyhow::ensure!(
        strength.is_finite() && (0.0..=1.0).contains(&strength),
        "invalid strength"
    );
    anyhow::ensure!(
        !std::path::Path::new(&args[2]).exists(),
        "output already exists"
    );
    let spec = schist_neural::spec("anti-smudge").context("missing model specification")?;
    let bytes = std::fs::read(&args[0])?;
    let model = schist_neural::Model::from_bytes(spec, &bytes)?;
    let mut image = image::open(&args[1])?.to_rgba32f();
    let (w, h) = (image.width() as usize, image.height() as usize);
    let mut rgb: Vec<f32> = image
        .pixels()
        .flat_map(|p| p.0[..3].iter().copied())
        .collect();
    schist_neural::try_restore(&model, &mut rgb, w, h, strength)?;
    for (pixel, result) in image.pixels_mut().zip(rgb.chunks_exact(3)) {
        if pixel.0[3] > 0.0 {
            pixel.0[..3].copy_from_slice(result);
        }
    }
    image::DynamicImage::ImageRgba32F(image)
        .to_rgba8()
        .save(&args[2])?;
    println!(
        "Wrote {} ({}×{}). Experimental weights; inspect restoration quality.",
        args[2], w, h
    );
    Ok(())
}
