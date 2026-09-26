//! Portable 3D asset import and depth-buffered rendering into document tiles.
mod import;
#[cfg(not(target_arch = "wasm32"))]
pub mod reconstruction;
mod render;
pub use import::import;
pub use render::render;
pub use schist_core::model3d::*;

pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(crate) fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
pub(crate) fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn normal(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt().max(1e-10);
    a.map(|v| v / l)
}

pub fn layer(
    model: &Model3d,
    depth: schist_color::Depth,
    canvas: schist_core::IntRect,
    name: &str,
) -> anyhow::Result<schist_core::Layer> {
    let mut layer = schist_core::Layer::new_raster(name);
    layer.as_raster_mut().unwrap().tiles = render(model, depth, canvas)?;
    layer.extras = model.blocks(&layer);
    Ok(layer)
}
