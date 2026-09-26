use super::*;
use anyhow::{ensure, Result};
use schist_color::{Depth, Rgba};
use schist_core::{IntRect, TileMap};
use schist_i18n::t;

fn rotate(mut p: [f32; 3], angles: [f32; 3]) -> [f32; 3] {
    for (axis, angle) in angles.into_iter().enumerate() {
        let (s, c) = angle.to_radians().sin_cos();
        let a = (axis + 1) % 3;
        let b = (axis + 2) % 3;
        (p[a], p[b]) = (c * p[a] - s * p[b], s * p[a] + c * p[b]);
    }
    p
}
struct Projected {
    point: [f32; 3],
    normal: [f32; 3],
    color: [f32; 4],
    uv: [f32; 2],
    inverse_z: f32,
}
fn edge(a: [f32; 3], b: [f32; 3], x: f32, y: f32) -> f32 {
    (b[0] - a[0]) * (y - a[1]) - (b[1] - a[1]) * (x - a[0])
}

/// Orthographic or perspective projection with a depth buffer, smooth normals,
/// vertex colors, texture coordinates and independently movable lighting.
pub fn render(model: &Model3d, depth: Depth, canvas: IntRect) -> Result<TileMap> {
    ensure!(model.valid(), t("model3d.error.invalid"));
    let placement = &model.placement;
    let mut bounds = IntRect::EMPTY;
    let vertices: Vec<_> = model
        .mesh
        .vertices
        .iter()
        .map(|v| {
            let p = rotate(v.position, placement.rotation);
            let inverse_z = if placement.perspective {
                1.0 / (3.0 - p[2]).max(0.01)
            } else {
                1.0
            };
            let projection = if placement.perspective {
                inverse_z * 3.0
            } else {
                1.0
            };
            let (x, y) = placement.transform.apply(
                placement.center[0] + p[0] * placement.scale * projection,
                placement.center[1] - p[1] * placement.scale * projection,
            );
            let point = [x, y, 3.0 - p[2]];
            bounds = bounds.union(&IntRect::new(
                (point[0].floor() as i32).saturating_sub(1),
                (point[1].floor() as i32).saturating_sub(1),
                (point[0].ceil() as i32).saturating_add(1),
                (point[1].ceil() as i32).saturating_add(1),
            ));
            Projected {
                point,
                normal: normal(rotate(v.normal, placement.rotation)),
                color: v.color,
                uv: v.uv,
                inverse_z,
            }
        })
        .collect();
    let bounds = bounds.intersect(&canvas);
    if bounds.is_empty() {
        return Ok(TileMap::new());
    }
    let pixels = bounds.width() as usize * bounds.height() as usize;
    ensure!(pixels <= 16_777_216, t("model3d.error.render_limit"));
    let samples = if pixels <= 4_194_304 { 4 } else { 1 };
    let mut zbuffer = vec![f32::INFINITY; pixels * samples];
    let mut rgba = vec![Rgba::TRANSPARENT; pixels * samples];
    let (az, el) = (
        placement.light_azimuth.to_radians(),
        placement.light_elevation.to_radians(),
    );
    let light = [az.sin() * el.cos(), el.sin(), az.cos() * el.cos()];
    for triangle in &model.mesh.triangles {
        let [a, b, c] = triangle.vertices.map(|i| &vertices[i as usize]);
        let area = edge(a.point, b.point, c.point[0], c.point[1]);
        if area.abs() < 1e-8 {
            continue;
        }
        let points = [a.point, b.point, c.point];
        let rect = IntRect::new(
            points.iter().map(|p| p[0].floor() as i32).min().unwrap(),
            points.iter().map(|p| p[1].floor() as i32).min().unwrap(),
            points.iter().map(|p| p[0].ceil() as i32).max().unwrap(),
            points.iter().map(|p| p[1].ceil() as i32).max().unwrap(),
        )
        .intersect(&bounds);
        let texture = triangle.texture.map(|i| &model.mesh.textures[i]);
        for y in rect.top..rect.bottom {
            for x in rect.left..rect.right {
                for sample in 0..samples {
                    let (ox, oy) = if samples == 1 {
                        (0.5, 0.5)
                    } else {
                        ([(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)])[sample]
                    };
                    let px = x as f32 + ox;
                    let py = y as f32 + oy;
                    let mut weights = [
                        edge(b.point, c.point, px, py) / area,
                        edge(c.point, a.point, px, py) / area,
                        edge(a.point, b.point, px, py) / area,
                    ];
                    if weights.iter().any(|&w| w < -1e-6) {
                        continue;
                    }
                    // Perspective-correct interpolation for depth, color and UVs.
                    let inv = weights[0] * a.inverse_z
                        + weights[1] * b.inverse_z
                        + weights[2] * c.inverse_z;
                    for (w, v) in weights.iter_mut().zip([a, b, c]) {
                        *w = *w * v.inverse_z / inv;
                    }
                    let z =
                        weights[0] * a.point[2] + weights[1] * b.point[2] + weights[2] * c.point[2];
                    let index = ((y - bounds.top) as usize * bounds.width() as usize
                        + (x - bounds.left) as usize)
                        * samples
                        + sample;
                    if z >= zbuffer[index] {
                        continue;
                    }
                    let n = normal(std::array::from_fn(|i| {
                        weights[0] * a.normal[i]
                            + weights[1] * b.normal[i]
                            + weights[2] * c.normal[i]
                    }));
                    let lighting =
                        placement.ambient + placement.light_intensity * dot(n, light).max(0.0);
                    let mut color: [f32; 4] = std::array::from_fn(|i| {
                        weights[0] * a.color[i] + weights[1] * b.color[i] + weights[2] * c.color[i]
                    });
                    if let Some(texture) = texture {
                        let uv: [f32; 2] = std::array::from_fn(|i| {
                            weights[0] * a.uv[i] + weights[1] * b.uv[i] + weights[2] * c.uv[i]
                        });
                        let tx = uv[0].rem_euclid(1.0) * texture.width as f32 - 0.5;
                        let ty = uv[1].rem_euclid(1.0) * texture.height as f32 - 0.5;
                        let (fx, fy) = (tx - tx.floor(), ty - ty.floor());
                        for (channel, value) in color.iter_mut().enumerate() {
                            let texel = |dx: i32, dy: i32| {
                                let x = (tx.floor() as i32 + dx).rem_euclid(texture.width as i32)
                                    as usize;
                                let y = (ty.floor() as i32 + dy).rem_euclid(texture.height as i32)
                                    as usize;
                                texture.rgba[(y * texture.width as usize + x) * 4 + channel] as f32
                                    / 255.0
                            };
                            *value *= (texel(0, 0) * (1.0 - fx) + texel(1, 0) * fx) * (1.0 - fy)
                                + (texel(0, 1) * (1.0 - fx) + texel(1, 1) * fx) * fy;
                        }
                    }
                    if color[3] <= 0.001 {
                        continue;
                    }
                    rgba[index] = Rgba::new(
                        (color[0] * lighting).clamp(0.0, 1.0),
                        (color[1] * lighting).clamp(0.0, 1.0),
                        (color[2] * lighting).clamp(0.0, 1.0),
                        color[3].clamp(0.0, 1.0),
                    );
                    zbuffer[index] = z;
                }
            }
        }
    }
    let mut output = Vec::with_capacity(pixels * 4);
    for samples in rgba.chunks_exact(samples) {
        let alpha = samples.iter().map(|p| p.a).sum::<f32>();
        for channel in 0..3 {
            output.push(if alpha > 1e-8 {
                samples
                    .iter()
                    .map(|p| [p.r, p.g, p.b][channel] * p.a)
                    .sum::<f32>()
                    / alpha
            } else {
                0.0
            });
        }
        output.push(alpha / samples.len() as f32);
    }
    let mut tiles = TileMap::new();
    schist_core::blit_rgba_f32(&mut tiles, depth, bounds, &output);
    Ok(tiles)
}
