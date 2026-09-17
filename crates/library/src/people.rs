//! The desktop People pipeline, with models owned by the caller's instance.
use anyhow::{ensure, Context, Result};
pub use schist_neural::FaceRect;
use schist_neural::Model;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct PeopleRequest {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    #[serde(default)]
    pub boxes: Option<Vec<FaceRect>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Face {
    pub rect: FaceRect,
    pub embedding: Vec<f32>,
}

#[derive(Default)]
pub struct People {
    detector: Option<Model>,
    recogniser: Option<Model>,
}

impl People {
    /// Load caller-supplied ONNX bytes. A failed replacement keeps the old model.
    /// No implicit download, environment lookup, or process-wide model cache.
    pub fn load_model(&mut self, id: &str, bytes: &[u8]) -> Result<()> {
        ensure!(
            matches!(id, "face" | "face-embed"),
            "Expected face or face-embed model"
        );
        let spec = schist_neural::spec(id).context("Unknown model")?;
        let model = Model::from_bytes(spec, bytes)?;
        if id == "face" {
            self.detector = Some(model);
        } else {
            self.recogniser = Some(model);
        }
        Ok(())
    }

    pub fn unload_model(&mut self, id: &str) -> Result<()> {
        match id {
            "face" => self.detector = None,
            "face-embed" => self.recogniser = None,
            _ => anyhow::bail!("Expected face or face-embed model"),
        }
        Ok(())
    }

    pub fn detect(&self, width: u32, height: u32, rgb: &[u8]) -> Result<Vec<FaceRect>> {
        validate_pixels(width, height, rgb)?;
        let detector = self.detector.as_ref().context("Detection model missing")?;
        let pixels: Vec<f32> = rgb.iter().map(|v| *v as f32 / 255.0).collect();
        Ok(
            schist_neural::faces(detector, &pixels, width as usize, height as usize)?
                .into_iter()
                .take(100)
                .map(|f| {
                    FaceRect::from_pixels(f.x, f.y, f.width, f.height, width as f32, height as f32)
                })
                .collect(),
        )
    }

    /// Same request and response as the retired schist-people-worker executable.
    pub fn process(&self, request: PeopleRequest) -> Result<Vec<Face>> {
        validate_pixels(request.width, request.height, &request.rgb)?;
        let boxes = match request.boxes {
            Some(boxes) => {
                ensure!(boxes.len() <= 100, "Too many faces");
                ensure!(
                    boxes
                        .iter()
                        .all(|b| [b.x, b.y, b.w, b.h].iter().all(|v| v.is_finite())),
                    "Invalid face rectangle"
                );
                boxes.into_iter().map(FaceRect::clamped).collect()
            }
            None => self.detect(request.width, request.height, &request.rgb)?,
        };
        let image = image::RgbImage::from_raw(request.width, request.height, request.rgb)
            .context("Invalid pixels")?;
        let boxes: Vec<_> = boxes
            .into_iter()
            .filter(|r| r.w > 0.0 && r.h > 0.0)
            .collect();
        if boxes.is_empty() {
            return Ok(Vec::new());
        }
        let recogniser = self
            .recogniser
            .as_ref()
            .context("Recognition model missing")?;
        boxes
            .into_iter()
            .map(|rect| {
                let (x, y, side) = rect.crop_square(1.1, request.width, request.height);
                let crop = image::imageops::crop_imm(&image, x, y, side, side).to_image();
                let crop =
                    image::imageops::resize(&crop, 112, 112, image::imageops::FilterType::Triangle);
                let rgb: Vec<f32> = crop.as_raw().iter().map(|v| *v as f32 / 255.0).collect();
                let embedding = schist_neural::embed_face(recogniser, &rgb)?;
                ensure!(
                    embedding.len() == schist_neural::FACE_EMBED_DIM
                        && embedding.iter().all(|v| v.is_finite()),
                    "Invalid embedding"
                );
                Ok(Face { rect, embedding })
            })
            .collect()
    }
}

fn validate_pixels(width: u32, height: u32, rgb: &[u8]) -> Result<()> {
    ensure!(
        (1..=1024).contains(&width) && (1..=1024).contains(&height),
        "Invalid dimensions"
    );
    ensure!(
        rgb.len() == width as usize * height as usize * 3,
        "Invalid pixels"
    );
    Ok(())
}
