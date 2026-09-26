//! Retained 3D geometry and placement. Import and rendering live in model3d.
use crate::{Layer, RawBlock};
use serde::{Deserialize, Serialize};
pub const BLOCK: [u8; 4] = *b"sc3D";
pub const MAX_VERTICES: usize = 1_000_000;
pub const MAX_TRIANGLES: usize = 1_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 2],
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Triangle {
    pub vertices: [u32; 3],
    pub texture: Option<usize>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub triangles: Vec<Triangle>,
    pub textures: Vec<Texture>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    #[serde(default)]
    pub transform: crate::Affine,
    pub center: [f32; 2],
    /// Pixels per normalized model unit.
    pub scale: f32,
    /// Euler rotation in degrees, applied X, Y, Z.
    pub rotation: [f32; 3],
    pub light_azimuth: f32,
    pub light_elevation: f32,
    pub light_intensity: f32,
    pub ambient: f32,
    pub perspective: bool,
}
impl Placement {
    pub fn fitted(width: u32, height: u32) -> Self {
        Self {
            transform: crate::Affine::IDENTITY,
            center: [width as f32 * 0.5, height as f32 * 0.5],
            scale: width.min(height) as f32 * 0.38,
            rotation: [0.0; 3],
            light_azimuth: -35.0,
            light_elevation: 45.0,
            light_intensity: 0.8,
            ambient: 0.3,
            perspective: true,
        }
    }
    pub fn valid(&self) -> bool {
        [
            self.transform.a,
            self.transform.b,
            self.transform.c,
            self.transform.d,
            self.transform.tx,
            self.transform.ty,
        ]
        .iter()
        .all(|v| v.is_finite() && v.abs() < 1e7)
            && self
                .center
                .iter()
                .chain(self.rotation.iter())
                .chain([
                    &self.scale,
                    &self.light_azimuth,
                    &self.light_elevation,
                    &self.light_intensity,
                    &self.ambient,
                ])
                .all(|v| v.is_finite() && v.abs() <= 1e7)
            && self.scale > 0.0
            && (0.0..=8.0).contains(&self.light_intensity)
            && (0.0..=1.0).contains(&self.ambient)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Model3d {
    pub mesh: Mesh,
    pub placement: Placement,
}
impl Model3d {
    pub fn valid(&self) -> bool {
        self.placement.valid() && self.mesh.valid()
    }
    pub fn from_layer(layer: &Layer) -> Option<Self> {
        Self::from_blocks(&layer.extras)
    }
    pub(crate) fn from_blocks(blocks: &[RawBlock]) -> Option<Self> {
        let block = blocks.iter().find(|b| b.key == BLOCK)?;
        let bytes =
            miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&block.data, 256 * 1024 * 1024)
                .ok()?;
        let model: Self = serde_json::from_slice(&bytes).ok()?;
        model.valid().then_some(model)
    }
    pub fn blocks(&self, layer: &Layer) -> Vec<RawBlock> {
        self.replace_blocks(&layer.extras)
    }
    pub(crate) fn replace_blocks(&self, blocks: &[RawBlock]) -> Vec<RawBlock> {
        let mut blocks = blocks.to_vec();
        blocks.retain(|b| b.key != BLOCK);
        blocks.push(RawBlock {
            key: BLOCK,
            data: miniz_oxide::deflate::compress_to_vec_zlib(
                &serde_json::to_vec(self).expect("finite 3D model"),
                1,
            ),
        });
        blocks
    }
}
impl Mesh {
    pub fn valid(&self) -> bool {
        !self.vertices.is_empty()
            && !self.triangles.is_empty()
            && self.vertices.len() <= MAX_VERTICES
            && self.triangles.len() <= MAX_TRIANGLES
            && self.vertices.iter().all(|v| {
                v.position
                    .iter()
                    .chain(v.normal.iter())
                    .chain(v.color.iter())
                    .chain(v.uv.iter())
                    .all(|v| v.is_finite() && v.abs() < 1e7)
            })
            && self.triangles.iter().all(|t| {
                t.vertices
                    .iter()
                    .all(|&i| (i as usize) < self.vertices.len())
                    && t.texture.is_none_or(|i| i < self.textures.len())
            })
            && self.textures.len() <= 64
            && self.textures.iter().all(|t| {
                t.width > 0
                    && t.height > 0
                    && t.width <= 8192
                    && t.height <= 8192
                    && (t.width as usize * t.height as usize * 4) == t.rgba.len()
            })
            && self.textures.iter().map(|t| t.rgba.len()).sum::<usize>() <= 64 * 1024 * 1024
    }
    /// Center and fit arbitrary asset units to a unit sphere. The placement
    /// then has consistent drag/scale semantics for OBJ, STL and glTF alike.
    pub fn normalize(&mut self) {
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for v in &self.vertices {
            for i in 0..3 {
                lo[i] = lo[i].min(v.position[i]);
                hi[i] = hi[i].max(v.position[i]);
            }
        }
        let c = std::array::from_fn::<_, 3, _>(|i| (lo[i] + hi[i]) * 0.5);
        let radius = self
            .vertices
            .iter()
            .map(|v| {
                (0..3)
                    .map(|i| (v.position[i] - c[i]).powi(2))
                    .sum::<f32>()
                    .sqrt()
            })
            .fold(1e-6, f32::max);
        for v in &mut self.vertices {
            for (i, center) in c.iter().enumerate() {
                v.position[i] = (v.position[i] - center) / radius;
            }
        }
    }
}
