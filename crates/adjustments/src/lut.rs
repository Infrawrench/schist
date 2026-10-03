//! Colour lookup tables: `.cube` and `.3dl` parsing, `.cube` writing and
//! tetrahedral interpolation.
//!
//! The formats are read from their public descriptions: Adobe's *Cube LUT
//! Specification 1.0* (`TITLE`, `LUT_1D_SIZE`, `LUT_3D_SIZE`,
//! `DOMAIN_MIN`/`DOMAIN_MAX`, red varying fastest) with the
//! `LUT_1D_INPUT_RANGE`/`LUT_3D_INPUT_RANGE` spelling and the 1D-shaper-
//! then-3D layout that Resolve writes, and the Autodesk/Lustre `.3dl`
//! layout: a line of input mesh points, then integer triples with blue
//! varying fastest, scaled by the output bit depth.
//!
//! Everything here is untrusted input. Sizes are bounded, entry counts
//! must match what the header declared, and nothing is allocated from a
//! declared size until the entries have actually arrived.

use std::fmt;

/// Largest 3D lattice accepted. Adobe's specification allows 256, but a
/// 256³ table is 200 MB of floats; 129 covers every size in practical use
/// (17, 32, 33, 64, 65) with room to spare.
pub const MAX_3D_SIZE: usize = 129;
/// Largest 1D table accepted, the specification's own limit.
pub const MAX_1D_SIZE: usize = 65_536;
/// Largest LUT file accepted. A 129³ `.cube` is about 60 MB of text.
pub const MAX_FILE_BYTES: usize = 128 << 20;

/// Why a file is not a LUT we can use. Logged, not shown: the chrome
/// reports a single translated "not a LUT" message.
#[derive(Debug, Clone, PartialEq)]
pub enum LutError {
    TooLarge,
    NotText,
    /// A line that is neither a keyword nor three numbers.
    Syntax(usize),
    /// A table size outside the supported range.
    Size(usize),
    /// The number of entries does not match the declared size.
    Count {
        expected: usize,
        found: usize,
    },
    /// `DOMAIN_MIN` is not below `DOMAIN_MAX` on some axis.
    Domain,
    /// No table at all.
    Empty,
}

impl fmt::Display for LutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LutError::TooLarge => write!(f, "file is larger than {MAX_FILE_BYTES} bytes"),
            LutError::NotText => write!(f, "not a text file"),
            LutError::Syntax(line) => write!(f, "unreadable line {line}"),
            LutError::Size(n) => write!(f, "unsupported table size {n}"),
            LutError::Count { expected, found } => {
                write!(f, "expected {expected} entries, found {found}")
            }
            LutError::Domain => write!(f, "domain minimum is not below its maximum"),
            LutError::Empty => write!(f, "no table"),
        }
    }
}

impl std::error::Error for LutError {}

/// A per-channel 1D table over `domain_min..=domain_max`.
#[derive(Debug, Clone, PartialEq)]
pub struct Lut1d {
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub table: Vec<[f32; 3]>,
}

/// A 3D lattice over `domain_min..=domain_max`, `size` points per axis,
/// red varying fastest (the `.cube` order).
#[derive(Debug, Clone, PartialEq)]
pub struct Lut3d {
    pub size: usize,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    pub table: Vec<[f32; 3]>,
}

/// A loaded table: an optional 1D shaper applied first, then an optional
/// 3D lattice. At least one is present.
#[derive(Debug, Clone, PartialEq)]
pub struct Lut {
    pub title: Option<String>,
    pub shaper: Option<Lut1d>,
    pub cube: Option<Lut3d>,
}

impl Lut1d {
    pub fn size(&self) -> usize {
        self.table.len()
    }

    /// Linear interpolation per channel, clamped to the domain.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.table.len();
        let mut out = [0.0; 3];
        for c in 0..3 {
            let t = normalise(rgb[c], self.domain_min[c], self.domain_max[c]);
            let x = t * (n - 1) as f32;
            let i = (x as usize).min(n - 2);
            let f = x - i as f32;
            out[c] = self.table[i][c] + (self.table[i + 1][c] - self.table[i][c]) * f;
        }
        out
    }
}

impl Lut3d {
    /// The identity lattice: each point maps to its own coordinates.
    pub fn identity(size: usize) -> Lut3d {
        let size = size.clamp(2, MAX_3D_SIZE);
        let step = 1.0 / (size - 1) as f32;
        let mut table = Vec::with_capacity(size * size * size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    table.push([r as f32 * step, g as f32 * step, b as f32 * step]);
                }
            }
        }
        Lut3d {
            size,
            domain_min: [0.0; 3],
            domain_max: [1.0; 3],
            table,
        }
    }

    #[inline]
    fn at(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        self.table[r + self.size * (g + self.size * b)]
    }

    /// Tetrahedral interpolation: the cube cell around the point is split
    /// into six tetrahedra along its neutral diagonal, and the point is
    /// weighted across the four corners of the one containing it. Unlike
    /// trilinear interpolation, greys stay on the grey axis.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let mut base = [0usize; 3];
        let mut frac = [0.0f32; 3];
        for c in 0..3 {
            let x = normalise(rgb[c], self.domain_min[c], self.domain_max[c]) * (n - 1) as f32;
            let i = (x as usize).min(n - 2);
            base[c] = i;
            frac[c] = x - i as f32;
        }
        let [r, g, b] = base;
        let [fr, fg, fb] = frac;
        let c000 = self.at(r, g, b);
        let c111 = self.at(r + 1, g + 1, b + 1);
        // Walk from c000 to c111 along the edges of the tetrahedron the
        // point lies in, ordered by which fraction is largest.
        let (w, p1, p2) = if fr > fg {
            if fg > fb {
                ([fr, fg, fb], self.at(r + 1, g, b), self.at(r + 1, g + 1, b))
            } else if fr > fb {
                ([fr, fb, fg], self.at(r + 1, g, b), self.at(r + 1, g, b + 1))
            } else {
                ([fb, fr, fg], self.at(r, g, b + 1), self.at(r + 1, g, b + 1))
            }
        } else if fb > fg {
            ([fb, fg, fr], self.at(r, g, b + 1), self.at(r, g + 1, b + 1))
        } else if fb > fr {
            ([fg, fb, fr], self.at(r, g + 1, b), self.at(r, g + 1, b + 1))
        } else {
            ([fg, fr, fb], self.at(r, g + 1, b), self.at(r + 1, g + 1, b))
        };
        let mut out = [0.0; 3];
        for c in 0..3 {
            out[c] = c000[c]
                + w[0] * (p1[c] - c000[c])
                + w[1] * (p2[c] - p1[c])
                + w[2] * (c111[c] - p2[c]);
        }
        out
    }
}

impl Lut {
    /// Run a colour through the shaper and then the lattice.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let mut v = rgb;
        if let Some(shaper) = &self.shaper {
            v = shaper.apply(v);
        }
        if let Some(cube) = &self.cube {
            v = cube.apply(v);
        }
        v
    }

    /// A short description for the UI and logs, e.g. `33³`.
    pub fn size_label(&self) -> String {
        match (&self.shaper, &self.cube) {
            (_, Some(cube)) => format!("{}\u{b3}", cube.size),
            (Some(shaper), None) => format!("{}", shaper.size()),
            (None, None) => String::new(),
        }
    }
}

/// Position of `v` across `min..=max` as 0..=1. Non-finite input lands on
/// the low end rather than poisoning the interpolation.
#[inline]
fn normalise(v: f32, min: f32, max: f32) -> f32 {
    let t = (v - min) / (max - min);
    if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn text(bytes: &[u8]) -> Result<&str, LutError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(LutError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| LutError::NotText)?;
    // A UTF-8 byte-order mark is harmless; anything else binary is not.
    Ok(text.strip_prefix('\u{feff}').unwrap_or(text))
}

fn numbers<const N: usize>(fields: &[&str], line: usize) -> Result<[f32; N], LutError> {
    if fields.len() != N {
        return Err(LutError::Syntax(line));
    }
    let mut out = [0.0; N];
    for (slot, field) in out.iter_mut().zip(fields) {
        let v: f32 = field.parse().map_err(|_| LutError::Syntax(line))?;
        if !v.is_finite() {
            return Err(LutError::Syntax(line));
        }
        *slot = v;
    }
    Ok(out)
}

fn size_value(fields: &[&str], line: usize, max: usize) -> Result<usize, LutError> {
    let [n] = fields else {
        return Err(LutError::Syntax(line));
    };
    let n: usize = n.parse().map_err(|_| LutError::Syntax(line))?;
    if !(2..=max).contains(&n) {
        return Err(LutError::Size(n));
    }
    Ok(n)
}

/// Parse a `.cube` file.
pub fn parse_cube(bytes: &[u8]) -> Result<Lut, LutError> {
    let text = text(bytes)?;
    let mut title = None;
    let mut size_1d = None;
    let mut size_3d = None;
    let mut domain_min = None;
    let mut domain_max = None;
    let mut range_1d = None;
    let mut range_3d = None;
    let mut entries: Vec<[f32; 3]> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let first = content.as_bytes()[0];
        if first.is_ascii_alphabetic() {
            let mut fields = content.split_whitespace();
            let keyword = fields.next().unwrap_or("");
            let rest: Vec<&str> = fields.collect();
            match keyword {
                "TITLE" => {
                    let value = content["TITLE".len()..].trim();
                    title = Some(value.trim_matches('"').to_string());
                }
                "LUT_1D_SIZE" => size_1d = Some(size_value(&rest, line, MAX_1D_SIZE)?),
                "LUT_3D_SIZE" => size_3d = Some(size_value(&rest, line, MAX_3D_SIZE)?),
                "DOMAIN_MIN" => domain_min = Some(numbers::<3>(&rest, line)?),
                "DOMAIN_MAX" => domain_max = Some(numbers::<3>(&rest, line)?),
                "LUT_1D_INPUT_RANGE" => range_1d = Some(numbers::<2>(&rest, line)?),
                "LUT_3D_INPUT_RANGE" => range_3d = Some(numbers::<2>(&rest, line)?),
                // Other writers' keywords (LUT_IN_VIDEO_RANGE and the like)
                // describe how a host should use the table, not the table.
                _ => {
                    if !entries.is_empty() {
                        return Err(LutError::Syntax(line));
                    }
                }
            }
            continue;
        }
        if size_1d.is_none() && size_3d.is_none() {
            return Err(LutError::Syntax(line));
        }
        let fields: Vec<&str> = content.split_whitespace().collect();
        entries.push(numbers::<3>(&fields, line)?);
        let declared = size_1d.unwrap_or(0) + size_3d.map_or(0, |n: usize| n * n * n);
        if entries.len() > declared {
            return Err(LutError::Count {
                expected: declared,
                found: entries.len(),
            });
        }
    }
    let n1 = size_1d.unwrap_or(0);
    let n3 = size_3d.map_or(0, |n| n * n * n);
    if n1 + n3 == 0 {
        return Err(LutError::Empty);
    }
    if entries.len() != n1 + n3 {
        return Err(LutError::Count {
            expected: n1 + n3,
            found: entries.len(),
        });
    }
    let domain = |range: Option<[f32; 2]>| -> Result<([f32; 3], [f32; 3]), LutError> {
        let (lo, hi) = match range {
            Some([lo, hi]) => ([lo; 3], [hi; 3]),
            None => (
                domain_min.unwrap_or([0.0; 3]),
                domain_max.unwrap_or([1.0; 3]),
            ),
        };
        if (0..3).any(|c| lo[c] >= hi[c]) {
            return Err(LutError::Domain);
        }
        Ok((lo, hi))
    };
    let mut cube_entries = entries.split_off(n1);
    let shaper = if n1 > 0 {
        let (domain_min, domain_max) = domain(range_1d)?;
        Some(Lut1d {
            domain_min,
            domain_max,
            table: entries,
        })
    } else {
        None
    };
    let cube = match size_3d {
        Some(size) => {
            // With a shaper in front, the lattice is addressed by the
            // shaper's output; Resolve states that range separately.
            let (domain_min, domain_max) = if shaper.is_some() && range_3d.is_none() {
                ([0.0; 3], [1.0; 3])
            } else {
                domain(range_3d)?
            };
            cube_entries.shrink_to_fit();
            Some(Lut3d {
                size,
                domain_min,
                domain_max,
                table: cube_entries,
            })
        }
        None => None,
    };
    Ok(Lut {
        title: title.filter(|t| !t.is_empty()),
        shaper,
        cube,
    })
}

/// Parse a `.3dl` file (Autodesk Lustre/Flame and the many tools that
/// write the same layout).
pub fn parse_3dl(bytes: &[u8]) -> Result<Lut, LutError> {
    let text = text(bytes)?;
    let mut mesh: Option<Vec<f32>> = None;
    let mut out_bits: Option<u32> = None;
    let mut entries: Vec<[f32; 3]> = Vec::new();
    let mut expected = 0usize;
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let fields: Vec<&str> = content.split_whitespace().collect();
        if fields[0].parse::<f32>().is_err() && fields[0].bytes().any(|b| b.is_ascii_alphabetic()) {
            // Lustre's header: `3DMESH` then `Mesh <in bits> <out bits>`.
            // Other keywords (`LUT8`, `gamma 1.0`, …) do not change how the
            // table is read.
            if fields[0].eq_ignore_ascii_case("mesh") && fields.len() == 3 {
                out_bits = fields[2].parse().ok().filter(|b| (8..=32).contains(b));
            }
            continue;
        }
        if mesh.is_none() {
            // The first numeric line is the input mesh.
            let values: Result<Vec<f32>, _> = fields.iter().map(|f| f.parse::<f32>()).collect();
            let values = values.map_err(|_| LutError::Syntax(line))?;
            let n = values.len();
            if !(2..=MAX_3D_SIZE).contains(&n) {
                return Err(LutError::Size(n));
            }
            if values.iter().any(|v| !v.is_finite())
                || values.windows(2).any(|w| w[0] >= w[1])
                || values[0] < 0.0
            {
                return Err(LutError::Syntax(line));
            }
            expected = n * n * n;
            mesh = Some(values);
            continue;
        }
        entries.push(numbers::<3>(&fields, line)?);
        if entries.len() > expected {
            return Err(LutError::Count {
                expected,
                found: entries.len(),
            });
        }
    }
    let mesh = mesh.ok_or(LutError::Empty)?;
    if entries.len() != expected {
        return Err(LutError::Count {
            expected,
            found: entries.len(),
        });
    }
    let n = mesh.len();
    // Input depth from the mesh's last point, output depth from the
    // header or else from the largest value present: the usual 10-, 12-
    // and 16-bit ranges, or 0..1 floats.
    let in_max = code_range(mesh[n - 1]);
    let peak = entries.iter().flatten().fold(0.0f32, |m, v| m.max(*v));
    let out_max = match out_bits {
        Some(bits) => ((1u64 << bits) - 1) as f32,
        None => code_range(peak),
    };
    // Reorder blue-fastest into the red-fastest lattice used everywhere
    // else.
    let mut table = vec![[0.0f32; 3]; expected];
    for (i, v) in entries.iter().enumerate() {
        let b = i % n;
        let g = (i / n) % n;
        let r = i / (n * n);
        table[r + n * (g + n * b)] = [v[0] / out_max, v[1] / out_max, v[2] / out_max];
    }
    // An evenly spaced mesh addresses the lattice directly. Anything else
    // (Lustre's 0, 64, …, 960, 1023 is the common case) goes through a
    // shaper mapping input to lattice position.
    let positions: Vec<f32> = mesh.iter().map(|v| v / in_max).collect();
    let uniform = positions
        .iter()
        .enumerate()
        .all(|(i, p)| (p - i as f32 / (n - 1) as f32).abs() < 1e-6);
    let shaper = (!uniform).then(|| mesh_shaper(&positions));
    Ok(Lut {
        title: None,
        shaper,
        cube: Some(Lut3d {
            size: n,
            domain_min: [0.0; 3],
            domain_max: [1.0; 3],
            table,
        }),
    })
}

/// The integer range a value belongs to: 0..1 floats, or 10/12/16-bit.
fn code_range(peak: f32) -> f32 {
    if peak <= 1.0 {
        1.0
    } else if peak <= 1023.0 {
        1023.0
    } else if peak <= 4095.0 {
        4095.0
    } else {
        65535.0
    }
}

/// A 1D table that maps an input in 0..=1 to its position between the
/// mesh points, as a fraction of the lattice.
fn mesh_shaper(positions: &[f32]) -> Lut1d {
    const SIZE: usize = 4096;
    let n = positions.len();
    let mut table = Vec::with_capacity(SIZE);
    for i in 0..SIZE {
        let x = i as f32 / (SIZE - 1) as f32;
        let k = positions
            .windows(2)
            .position(|w| x <= w[1])
            .unwrap_or(n - 2);
        let (a, b) = (positions[k], positions[k + 1]);
        let f = ((x - a) / (b - a)).clamp(0.0, 1.0);
        let v = (k as f32 + f) / (n - 1) as f32;
        table.push([v; 3]);
    }
    Lut1d {
        domain_min: [0.0; 3],
        domain_max: [1.0; 3],
        table,
    }
}

/// Write a 3D lattice as a `.cube` file.
pub fn write_cube(title: &str, cube: &Lut3d) -> Vec<u8> {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(64 + cube.table.len() * 30);
    let title = title.replace(['"', '\n', '\r'], "");
    if !title.is_empty() {
        let _ = writeln!(out, "TITLE \"{title}\"");
    }
    let _ = writeln!(out, "LUT_3D_SIZE {}", cube.size);
    if cube.domain_min != [0.0; 3] || cube.domain_max != [1.0; 3] {
        let [a, b, c] = cube.domain_min;
        let _ = writeln!(out, "DOMAIN_MIN {a} {b} {c}");
        let [a, b, c] = cube.domain_max;
        let _ = writeln!(out, "DOMAIN_MAX {a} {b} {c}");
    }
    for [r, g, b] in &cube.table {
        let _ = writeln!(out, "{r:.6} {g:.6} {b:.6}");
    }
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3], tol: f32) -> bool {
        (0..3).all(|c| (a[c] - b[c]).abs() <= tol)
    }

    fn cube_text(size: usize, f: impl Fn([f32; 3]) -> [f32; 3]) -> String {
        let mut s = format!("TITLE \"test\"\n# a comment\nLUT_3D_SIZE {size}\n\n");
        let step = 1.0 / (size - 1) as f32;
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    let [x, y, z] = f([r as f32 * step, g as f32 * step, b as f32 * step]);
                    s.push_str(&format!("{x} {y} {z}\n"));
                }
            }
        }
        s
    }

    #[test]
    fn identity_cube_is_identity_everywhere() {
        let lut = parse_cube(cube_text(17, |c| c).as_bytes()).unwrap();
        assert_eq!(lut.title.as_deref(), Some("test"));
        for i in 0..=40 {
            let v = [
                i as f32 / 40.0,
                (i * 7 % 41) as f32 / 40.0,
                1.0 - i as f32 / 40.0,
            ];
            assert!(close(lut.apply(v), v, 1e-5), "{v:?} -> {:?}", lut.apply(v));
        }
    }

    #[test]
    fn tetrahedral_is_exact_for_affine_tables() {
        // Any affine map is reproduced exactly by tetrahedral (and
        // trilinear) interpolation, so a coarse 3-point lattice of a
        // channel swap plus offset must match at arbitrary points.
        let f = |[r, g, b]: [f32; 3]| [0.2 + 0.5 * b, g * 0.8, 0.1 + 0.3 * r + 0.6 * g];
        let lut = parse_cube(cube_text(3, f).as_bytes()).unwrap();
        for v in [[0.13, 0.77, 0.4], [0.9, 0.05, 0.61], [0.5, 0.5, 0.5]] {
            assert!(close(lut.apply(v), f(v), 1e-5));
        }
    }

    #[test]
    fn tetrahedral_keeps_greys_on_the_grey_axis() {
        // The lattice nodes on the diagonal map to grey; off-diagonal
        // nodes are wildly coloured. Tetrahedral interpolation only uses
        // the c000 and c111 corners for a grey input, so it stays grey,
        // where trilinear would drag in the coloured corners.
        let f = |[r, g, b]: [f32; 3]| {
            if r == g && g == b {
                [r, g, b]
            } else {
                [1.0 - r, b, g * 0.3]
            }
        };
        let lut = parse_cube(cube_text(5, f).as_bytes()).unwrap();
        for i in 0..=20 {
            let v = i as f32 / 20.0;
            let out = lut.apply([v; 3]);
            assert!(close(out, [v; 3], 1e-5), "grey {v} became {out:?}");
        }
    }

    #[test]
    fn known_lut_hits_its_nodes_and_interpolates_between() {
        let f = |[r, g, b]: [f32; 3]| [r * r, g.sqrt(), 1.0 - b];
        let lut = parse_cube(cube_text(33, f).as_bytes()).unwrap();
        // Exactly on a node.
        let node = [4.0 / 32.0, 16.0 / 32.0, 31.0 / 32.0];
        assert!(close(lut.apply(node), f(node), 1e-5));
        // Between nodes, near the true function.
        for v in [[0.31, 0.47, 0.83], [0.66, 0.12, 0.05]] {
            assert!(close(lut.apply(v), f(v), 2e-3), "{v:?}");
        }
    }

    #[test]
    fn domain_scales_the_input() {
        let text = "LUT_3D_SIZE 2\nDOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n\
                    0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";
        let lut = parse_cube(text.as_bytes()).unwrap();
        assert!(close(lut.apply([1.0, 0.5, 2.0]), [0.5, 0.25, 1.0], 1e-6));
        // Beyond the domain clamps to the edge.
        assert!(close(lut.apply([5.0, -1.0, 0.0]), [1.0, 0.0, 0.0], 1e-6));
    }

    #[test]
    fn one_dimensional_and_shaper_tables() {
        let text = "LUT_1D_SIZE 3\n0 0 0\n0.25 0.5 1\n1 1 1\n";
        let lut = parse_cube(text.as_bytes()).unwrap();
        assert!(lut.cube.is_none());
        assert!(close(lut.apply([0.5, 0.5, 0.5]), [0.25, 0.5, 1.0], 1e-6));
        assert!(close(
            lut.apply([0.25, 0.75, 0.0]),
            [0.125, 0.75, 0.0],
            1e-6
        ));

        // A shaper squaring the input in front of an inverting lattice.
        let mut text = String::from("LUT_1D_SIZE 2\nLUT_3D_SIZE 2\nLUT_1D_INPUT_RANGE 0 4\n");
        text.push_str("0 0 0\n1 1 1\n");
        for b in 0..2 {
            for g in 0..2 {
                for r in 0..2 {
                    text.push_str(&format!("{} {} {}\n", 1 - r, 1 - g, 1 - b));
                }
            }
        }
        let lut = parse_cube(text.as_bytes()).unwrap();
        assert!(lut.shaper.is_some() && lut.cube.is_some());
        assert!(close(lut.apply([1.0, 2.0, 4.0]), [0.75, 0.5, 0.0], 1e-6));
    }

    #[test]
    fn malformed_cubes_are_refused() {
        let bad: &[(&str, LutError)] = &[
            ("", LutError::Empty),
            ("TITLE \"x\"\n", LutError::Empty),
            ("0 0 0\n", LutError::Syntax(1)),
            ("LUT_3D_SIZE 1\n", LutError::Size(1)),
            ("LUT_3D_SIZE 4096\n", LutError::Size(4096)),
            ("LUT_3D_SIZE two\n", LutError::Syntax(1)),
            (
                "LUT_3D_SIZE 2\n0 0 0\n",
                LutError::Count {
                    expected: 8,
                    found: 1,
                },
            ),
            ("LUT_1D_SIZE 2\n0 0\n1 1 1\n", LutError::Syntax(2)),
            ("LUT_1D_SIZE 2\n0 0 nan\n1 1 1\n", LutError::Syntax(2)),
            (
                "LUT_1D_SIZE 2\n0 0 0\n1 1 1\n2 2 2\n",
                LutError::Count {
                    expected: 2,
                    found: 3,
                },
            ),
            (
                "LUT_1D_SIZE 2\nDOMAIN_MIN 1 0 0\n0 0 0\n1 1 1\n",
                LutError::Domain,
            ),
        ];
        for (text, error) in bad {
            assert_eq!(parse_cube(text.as_bytes()).as_ref(), Err(error), "{text:?}");
        }
        assert_eq!(parse_cube(&[0xff, 0xfe, 0x00]), Err(LutError::NotText));
        // A huge declared size with no data must fail on the count, not
        // by allocating the declared lattice up front.
        assert!(matches!(
            parse_cube(b"LUT_3D_SIZE 129\n0 0 0\n"),
            Err(LutError::Count { .. })
        ));
    }

    #[test]
    fn written_cubes_parse_back() {
        let cube = Lut3d::identity(5);
        let bytes = write_cube("round \"trip\"", &cube);
        let lut = parse_cube(&bytes).unwrap();
        assert_eq!(lut.title.as_deref(), Some("round trip"));
        assert_eq!(lut.cube.as_ref().unwrap().table, cube.table);
    }

    fn three_dl(
        mesh: &[u32],
        out_max: u32,
        header: &str,
        f: impl Fn([f32; 3]) -> [f32; 3],
    ) -> String {
        let n = mesh.len();
        let in_max = *mesh.last().unwrap() as f32;
        let mut s = String::from(header);
        s.push_str(
            &mesh
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        );
        s.push('\n');
        for r in 0..n {
            for g in 0..n {
                for b in 0..n {
                    let v = f([
                        mesh[r] as f32 / in_max,
                        mesh[g] as f32 / in_max,
                        mesh[b] as f32 / in_max,
                    ]);
                    s.push_str(&format!(
                        "{} {} {}\n",
                        (v[0] * out_max as f32).round(),
                        (v[1] * out_max as f32).round(),
                        (v[2] * out_max as f32).round()
                    ));
                }
            }
        }
        s
    }

    #[test]
    fn three_dl_reads_blue_fastest_and_scales_by_depth() {
        let swap = |[r, g, b]: [f32; 3]| [b, r, g];
        // Evenly spaced 12-bit-output mesh: no shaper needed.
        let mesh: Vec<u32> = (0..4).map(|i| i * 341).collect();
        let text = three_dl(&mesh, 4095, "3DMESH\nMesh 10 12\n", swap);
        let lut = parse_3dl(text.as_bytes()).unwrap();
        assert!(lut.shaper.is_none());
        for v in [[0.2, 0.6, 0.9], [1.0, 0.0, 0.5]] {
            assert!(
                close(lut.apply(v), swap(v), 1e-3),
                "{v:?} -> {:?}",
                lut.apply(v)
            );
        }
        // Lustre's uneven 0, 64, …, 1023 mesh, 10-bit output with no
        // header: depth is inferred and the shaper keeps greys exact.
        let mesh: Vec<u32> = (0..17).map(|i| (i * 64).min(1023)).collect();
        let text = three_dl(&mesh, 1023, "", |c| c);
        let lut = parse_3dl(text.as_bytes()).unwrap();
        assert!(lut.shaper.is_some());
        for i in 0..=50 {
            let v = i as f32 / 50.0;
            assert!(close(lut.apply([v; 3]), [v; 3], 2e-3), "{v}");
        }
    }

    #[test]
    fn malformed_3dls_are_refused() {
        assert_eq!(parse_3dl(b""), Err(LutError::Empty));
        assert_eq!(parse_3dl(b"0 512 256\n"), Err(LutError::Syntax(1)));
        assert_eq!(parse_3dl(b"0\n"), Err(LutError::Size(1)));
        assert!(matches!(
            parse_3dl(b"0 1023\n0 0 0\n"),
            Err(LutError::Count { .. })
        ));
        assert_eq!(parse_3dl(b"0 1023\n0 0 x\n"), Err(LutError::Syntax(2)));
    }
}
