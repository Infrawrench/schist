//! Developer smoke test: make render-model3d ARGS="model.glb preview.png 45"
use schist_color::Depth;
use schist_core::IntRect;
use schist_model3d::{import, render, Model3d, Placement};
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() >= 3, "input model, output PNG, optional yaw");
    let path = std::path::Path::new(&args[1]);
    let mesh = import(
        &std::fs::read(path)?,
        path.extension().and_then(|e| e.to_str()).unwrap_or("glb"),
    )?;
    eprintln!(
        "{} vertices, {} triangles",
        mesh.vertices.len(),
        mesh.triangles.len()
    );
    let mut model = Model3d {
        mesh,
        placement: Placement::fitted(512, 512),
    };
    model.placement.rotation[1] = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(0.0);
    let tiles = render(&model, Depth::Eight, IntRect::from_size(512, 512))?;
    let image = image::RgbaImage::from_fn(512, 512, |x, y| {
        image::Rgba(tiles.pixel(x as i32, y as i32).to_u8())
    });
    image.save(&args[2])?;
    Ok(())
}
