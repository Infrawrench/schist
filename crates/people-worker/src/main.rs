//! A bounded cloud adapter for desktop's People models. No network or credentials.
use anyhow::{ensure, Context, Result};
use schist_gallery::FaceRect;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
#[derive(Deserialize)]
struct Request {
    width: u32,
    height: u32,
    rgb: Vec<u8>,
    #[serde(default)]
    boxes: Option<Vec<FaceRect>>,
}
#[derive(Serialize)]
struct Face {
    rect: FaceRect,
    embedding: Vec<f32>,
}
fn run() -> Result<()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(24 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 24 * 1024 * 1024, "Input too large");
    let request: Request = serde_json::from_slice(&bytes)?;
    ensure!(
        (1..=1024).contains(&request.width) && (1..=1024).contains(&request.height),
        "Invalid dimensions"
    );
    let image = image::RgbImage::from_raw(request.width, request.height, request.rgb)
        .context("Invalid pixels")?;
    let boxes = if let Some(boxes) = request.boxes {
        ensure!(boxes.len() <= 100, "Too many faces");
        boxes.into_iter().map(FaceRect::clamped).collect()
    } else {
        let detector = schist_neural::get("face").context("Detection model missing")?;
        let rgb: Vec<f32> = image.as_raw().iter().map(|v| *v as f32 / 255.0).collect();
        schist_neural::faces(
            &detector,
            &rgb,
            request.width as usize,
            request.height as usize,
        )?
        .into_iter()
        .take(100)
        .map(|face| {
            FaceRect::from_pixels(
                face.x,
                face.y,
                face.width,
                face.height,
                request.width as f32,
                request.height as f32,
            )
        })
        .collect::<Vec<_>>()
    };
    let recogniser = schist_neural::get("face-embed").context("Recognition model missing")?;
    let mut faces = Vec::new();
    for rect in boxes {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            continue;
        }
        let (x, y, side) = rect.crop_square(1.1, request.width, request.height);
        let crop = image::imageops::crop_imm(&image, x, y, side, side).to_image();
        let crop = image::imageops::resize(&crop, 112, 112, image::imageops::FilterType::Triangle);
        let rgb: Vec<f32> = crop.as_raw().iter().map(|v| *v as f32 / 255.0).collect();
        let embedding = schist_neural::embed_face(&recogniser, &rgb)?;
        ensure!(embedding.iter().all(|v| v.is_finite()), "Invalid embedding");
        faces.push(Face { rect, embedding });
    }
    std::io::stdout().write_all(&serde_json::to_vec(&faces)?)?;
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!("People processing failed");
        std::process::exit(1);
    }
}
