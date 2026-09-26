use super::*;
use anyhow::{bail, ensure, Context, Result};
use schist_i18n::t;

pub fn import(bytes: &[u8], extension: &str) -> Result<Mesh> {
    ensure!(bytes.len() <= 128 * 1024 * 1024, t("model3d.error.limit"));
    let mut mesh = if bytes.starts_with(b"glTF") || extension.eq_ignore_ascii_case("glb") {
        glb(bytes)?
    } else if extension.eq_ignore_ascii_case("stl") {
        stl(bytes)?
    } else {
        obj(std::str::from_utf8(bytes).context(t("model3d.error.invalid"))?)?
    };
    ensure!(mesh.valid(), t("model3d.error.invalid"));
    mesh.normalize();
    Ok(mesh)
}
fn vertex(position: [f32; 3], normal: [f32; 3], color: [f32; 4], uv: [f32; 2]) -> Vertex {
    Vertex {
        position,
        normal,
        color,
        uv,
    }
}
fn obj(text: &str) -> Result<Mesh> {
    let mut mesh = Mesh::default();
    let mut positions = Vec::new();
    let mut colors = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut unique = std::collections::HashMap::new();
    let parse = |s: &str| s.parse::<f32>().context(t("model3d.error.invalid"));
    for line in text.lines() {
        let mut words = line.split('#').next().unwrap_or("").split_whitespace();
        let Some(kind) = words.next() else {
            continue;
        };
        let fields: Vec<_> = words.collect();
        match kind {
            "v" => {
                ensure!(
                    fields.len() >= 3 && positions.len() < MAX_VERTICES,
                    t("model3d.error.limit")
                );
                positions.push([parse(fields[0])?, parse(fields[1])?, parse(fields[2])?]);
                colors.push(if fields.len() >= 6 {
                    [parse(fields[3])?, parse(fields[4])?, parse(fields[5])?, 1.0]
                } else {
                    [0.72, 0.72, 0.76, 1.0]
                });
            }
            "vn" => {
                ensure!(
                    fields.len() >= 3 && normals.len() < MAX_VERTICES,
                    t("model3d.error.limit")
                );
                normals.push(normal([
                    parse(fields[0])?,
                    parse(fields[1])?,
                    parse(fields[2])?,
                ]));
            }
            "vt" => {
                ensure!(
                    fields.len() >= 2 && uvs.len() < MAX_VERTICES,
                    t("model3d.error.limit")
                );
                uvs.push([parse(fields[0])?, 1.0 - parse(fields[1])?]);
            }
            "f" => {
                ensure!(
                    (3..=4096).contains(&fields.len()),
                    t("model3d.error.invalid")
                );
                let index = |v: &str, n: usize| -> Result<usize> {
                    let i = v.parse::<i64>().context(t("model3d.error.invalid"))?;
                    let i = if i < 0 { n as i64 + i } else { i - 1 };
                    ensure!(i >= 0 && (i as usize) < n, t("model3d.error.invalid"));
                    Ok(i as usize)
                };
                let mut face = Vec::new();
                for field in fields {
                    let parts: Vec<_> = field.split('/').collect();
                    let p = index(parts[0], positions.len())?;
                    let uv = parts
                        .get(1)
                        .filter(|s| !s.is_empty())
                        .map(|s| index(s, uvs.len()))
                        .transpose()?;
                    let n = parts
                        .get(2)
                        .filter(|s| !s.is_empty())
                        .map(|s| index(s, normals.len()))
                        .transpose()?;
                    let key = (p, uv, n);
                    let id = if let Some(&i) = unique.get(&key) {
                        i
                    } else {
                        ensure!(mesh.vertices.len() < MAX_VERTICES, t("model3d.error.limit"));
                        let i = mesh.vertices.len() as u32;
                        mesh.vertices.push(vertex(
                            positions[p],
                            n.map(|n| normals[n]).unwrap_or([0.0; 3]),
                            colors[p],
                            uv.map(|u| uvs[u]).unwrap_or([0.0; 2]),
                        ));
                        unique.insert(key, i);
                        i
                    };
                    face.push(id);
                }
                // Ear clipping supports concave OBJ polygons without filling
                // outside their boundary (a triangle fan would do that).
                for vertices in triangulate(&face, &mesh.vertices)? {
                    mesh.triangles.push(Triangle {
                        vertices,
                        texture: None,
                    });
                }
                ensure!(
                    mesh.triangles.len() <= MAX_TRIANGLES,
                    t("model3d.error.limit")
                );
            }
            _ => {}
        }
    }
    fill_normals(&mut mesh);
    Ok(mesh)
}

fn triangulate(face: &[u32], vertices: &[Vertex]) -> Result<Vec<[u32; 3]>> {
    if face.len() == 3 {
        return Ok(vec![[face[0], face[1], face[2]]]);
    }
    let mut n = [0.0; 3];
    for i in 0..face.len() {
        let p = vertices[face[i] as usize].position;
        let q = vertices[face[(i + 1) % face.len()] as usize].position;
        n[0] += (p[1] - q[1]) * (p[2] + q[2]);
        n[1] += (p[2] - q[2]) * (p[0] + q[0]);
        n[2] += (p[0] - q[0]) * (p[1] + q[1]);
    }
    let axis = (0..3)
        .max_by(|&a, &b| n[a].abs().total_cmp(&n[b].abs()))
        .unwrap();
    let point = |id: u32| {
        let v = vertices[id as usize].position;
        [v[(axis + 1) % 3], v[(axis + 2) % 3]]
    };
    let edge = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let sign = n[axis].signum();
    let mut polygon = face.to_vec();
    let mut out = Vec::new();
    while polygon.len() > 3 {
        let count = polygon.len();
        let mut found = false;
        for i in 0..count {
            let ids = [
                polygon[(i + count - 1) % count],
                polygon[i],
                polygon[(i + 1) % count],
            ];
            let [a, b, c] = ids.map(point);
            if edge(a, b, c) * sign <= 1e-8 {
                continue;
            }
            if polygon.iter().filter(|id| !ids.contains(id)).any(|&id| {
                let p = point(id);
                edge(a, b, p) * sign >= 0.0
                    && edge(b, c, p) * sign >= 0.0
                    && edge(c, a, p) * sign >= 0.0
            }) {
                continue;
            }
            out.push(ids);
            polygon.remove(i);
            found = true;
            break;
        }
        ensure!(found, t("model3d.error.invalid"));
    }
    out.push([polygon[0], polygon[1], polygon[2]]);
    Ok(out)
}

fn fill_normals(mesh: &mut Mesh) {
    let mut sums = vec![[0.0; 3]; mesh.vertices.len()];
    for tri in &mesh.triangles {
        let [a, b, c] = tri.vertices.map(|i| mesh.vertices[i as usize].position);
        let n = cross(sub(b, a), sub(c, a));
        for &i in &tri.vertices {
            for (j, v) in n.iter().enumerate() {
                sums[i as usize][j] += v;
            }
        }
    }
    for (v, sum) in mesh.vertices.iter_mut().zip(sums) {
        if dot(v.normal, v.normal) < 1e-8 {
            v.normal = normal(sum);
        }
    }
}

fn stl(bytes: &[u8]) -> Result<Mesh> {
    let mut mesh = Mesh::default();
    let count = bytes
        .get(80..84)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize);
    if let Some(count) = count.filter(|&n| n <= MAX_TRIANGLES && 84 + n * 50 == bytes.len()) {
        ensure!(count <= MAX_VERTICES / 3, t("model3d.error.limit"));
        for face in bytes[84..].as_chunks::<50>().0 {
            let f = |i| f32::from_le_bytes(face[i..i + 4].try_into().unwrap());
            let n = [f(0), f(4), f(8)];
            let start = mesh.vertices.len() as u32;
            for i in [12, 24, 36] {
                mesh.vertices.push(vertex(
                    [f(i), f(i + 4), f(i + 8)],
                    normal(n),
                    [0.72, 0.72, 0.76, 1.0],
                    [0.0; 2],
                ));
            }
            mesh.triangles.push(Triangle {
                vertices: [start, start + 1, start + 2],
                texture: None,
            });
        }
    } else {
        let text = std::str::from_utf8(bytes).context(t("model3d.error.invalid"))?;
        for line in text.lines() {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.first() == Some(&"vertex") {
                ensure!(
                    fields.len() == 4 && mesh.vertices.len() < MAX_VERTICES,
                    t("model3d.error.invalid")
                );
                let mut p = [0.0; 3];
                for i in 0..3 {
                    p[i] = fields[i + 1].parse().context(t("model3d.error.invalid"))?;
                }
                mesh.vertices
                    .push(vertex(p, [0.0; 3], [0.72, 0.72, 0.76, 1.0], [0.0; 2]));
            }
        }
        ensure!(mesh.vertices.len() % 3 == 0, t("model3d.error.invalid"));
        for i in (0..mesh.vertices.len() as u32).step_by(3) {
            mesh.triangles.push(Triangle {
                vertices: [i, i + 1, i + 2],
                texture: None,
            });
        }
    }
    fill_normals(&mut mesh);
    Ok(mesh)
}

type Matrix = [[f32; 4]; 4];
fn product(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|c| std::array::from_fn(|r| (0..4).map(|i| a[i][r] * b[c][i]).sum()))
}
fn glb(bytes: &[u8]) -> Result<Mesh> {
    let gltf = gltf::Gltf::from_slice(bytes).context(t("model3d.error.invalid"))?;
    let blob = gltf.blob.as_deref().context(t("model3d.error.embedded"))?;
    ensure!(
        gltf.buffers()
            .all(|b| matches!(b.source(), gltf::buffer::Source::Bin) && b.length() <= blob.len()),
        t("model3d.error.embedded")
    );
    let mut mesh = Mesh::default();
    // glTF texture indices can share one image; keep that mapping explicit.
    let mut textures = std::collections::HashMap::new();
    for texture in gltf.textures() {
        let source = texture.source();
        let data = match source.source() {
            gltf::image::Source::View { view, .. } => blob
                .get(view.offset()..view.offset() + view.length())
                .context(t("model3d.error.invalid"))?,
            _ => bail!(t("model3d.error.embedded")),
        };
        let mut reader =
            image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format()?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode()?.to_rgba8();
        ensure!(
            mesh.textures.len() < 64
                && mesh.textures.iter().map(|t| t.rgba.len()).sum::<usize>() + image.len()
                    <= 64 * 1024 * 1024,
            t("model3d.error.limit")
        );
        textures.insert(texture.index(), mesh.textures.len());
        mesh.textures.push(Texture {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        });
    }
    fn visit(
        node: gltf::Node<'_>,
        parent: Matrix,
        blob: &[u8],
        textures: &std::collections::HashMap<usize, usize>,
        mesh: &mut Mesh,
        depth: usize,
    ) -> Result<()> {
        ensure!(depth < 128, t("model3d.error.limit"));
        let matrix = product(parent, node.transform().matrix());
        if let Some(asset) = node.mesh() {
            for primitive in asset.primitives() {
                ensure!(
                    primitive.mode() == gltf::mesh::Mode::Triangles,
                    t("model3d.error.triangles")
                );
                let reader = primitive.reader(|_| Some(blob));
                let positions = reader
                    .read_positions()
                    .context(t("model3d.error.invalid"))?;
                let count = positions.len();
                ensure!(
                    count + mesh.vertices.len() <= MAX_VERTICES,
                    t("model3d.error.limit")
                );
                let mut normals = reader.read_normals();
                let mut colors = reader.read_colors(0).map(|c| c.into_rgba_f32());
                let mut uv = reader.read_tex_coords(0).map(|u| u.into_f32());
                let material = primitive.material().pbr_metallic_roughness();
                let factor = material.base_color_factor();
                let texture = material
                    .base_color_texture()
                    .and_then(|t| textures.get(&t.texture().index()).copied());
                let offset = mesh.vertices.len() as u32;
                // Inverse transpose, including nonuniform scale and mirrored nodes.
                let columns = [
                    matrix[0][..3].try_into().unwrap(),
                    matrix[1][..3].try_into().unwrap(),
                    matrix[2][..3].try_into().unwrap(),
                ];
                let cofactor = [
                    cross(columns[1], columns[2]),
                    cross(columns[2], columns[0]),
                    cross(columns[0], columns[1]),
                ];
                let determinant = dot(columns[0], cofactor[0]);
                for p in positions {
                    let p = std::array::from_fn(|r| {
                        matrix[3][r] + (0..3).map(|c| matrix[c][r] * p[c]).sum::<f32>()
                    });
                    let n = normals
                        .as_mut()
                        .and_then(|n| n.next())
                        .map(|n| {
                            normal(std::array::from_fn(|r| {
                                (0..3)
                                    .map(|c| cofactor[c][r] * n[c] * determinant.signum())
                                    .sum()
                            }))
                        })
                        .unwrap_or([0.0; 3]);
                    let color = colors.as_mut().and_then(|c| c.next()).unwrap_or([1.0; 4]);
                    let color = std::array::from_fn(|i| color[i] * factor[i]);
                    mesh.vertices.push(vertex(
                        p,
                        n,
                        color,
                        uv.as_mut().and_then(|u| u.next()).unwrap_or([0.0; 2]),
                    ));
                }
                let indices: Vec<u32> = reader
                    .read_indices()
                    .map(|i| i.into_u32().collect())
                    .unwrap_or_else(|| (0..count as u32).collect());
                ensure!(
                    indices.len().is_multiple_of(3)
                        && indices.iter().all(|&i| (i as usize) < count)
                        && mesh.triangles.len() + indices.len() / 3 <= MAX_TRIANGLES,
                    t("model3d.error.invalid")
                );
                for i in indices.as_chunks::<3>().0 {
                    mesh.triangles.push(Triangle {
                        vertices: [i[0] + offset, i[1] + offset, i[2] + offset],
                        texture,
                    });
                }
            }
        }
        for child in node.children() {
            visit(child, matrix, blob, textures, mesh, depth + 1)?;
        }
        Ok(())
    }
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .context(t("model3d.error.invalid"))?;
    let identity = std::array::from_fn(|c| std::array::from_fn(|r| if c == r { 1.0 } else { 0.0 }));
    for root in scene.nodes() {
        visit(root, identity, blob, &textures, &mut mesh, 0)?;
    }
    fill_normals(&mut mesh);
    Ok(mesh)
}
