//! Portable, self-contained Lensfun v1/v2 rectilinear calibration recipes.
//!
//! This is an independent implementation of the documented Lensfun models,
//! not the Lensfun library or its interpolation algorithm. No database is bundled.
use crate::{
    param,
    util::{premultiply, sample, unpremultiply},
};
use schist_i18n::t;
use schist_plugin_api::{FilterParam, FilterValues};
use std::sync::{OnceLock, RwLock};

const MAX_XML: usize = 8 * 1024 * 1024;
#[cfg(not(target_arch = "wasm32"))]
const MAX_DATABASE: usize = 64 * 1024 * 1024;
/// Serialized numeric parameters: never save database ordinals or paths.
pub const KEYS: [&str; 15] = [
    "lp_enabled",
    "lp_id",
    "lp_focal",
    "lp_crop",
    "lp_cal_crop",
    "lp_aspect",
    "lp_model",
    "lp_a",
    "lp_b",
    "lp_c",
    "lp_d",
    "lp_br",
    "lp_cr",
    "lp_vr",
    "lp_vb",
];
const EXTRA_KEYS: [&str; 10] = [
    "lp_id_hi",
    "lp_bb",
    "lp_cb",
    "lp_aperture",
    "lp_distance",
    "lp_vignette",
    "lp_vk1",
    "lp_vk2",
    "lp_vk3",
    "lp_vig_available",
];

#[derive(Clone, Debug)]
struct Sample {
    focal: f32,
    model: u8,
    coefficients: [f32; 4],
}
#[derive(Clone, Debug)]
struct Vignette {
    focal: f32,
    aperture: f32,
    distance: f32,
    k: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct Profile {
    pub name: String,
    pub id: u64,
    maker: String,
    model: String,
    mounts: Vec<String>,
    crop: f32,
    aspect: f32,
    distortion: Vec<Sample>,
    red: Vec<Sample>,
    blue: Vec<Sample>,
    vignette: Vec<Vignette>,
}
#[derive(Clone, Debug)]
struct Camera {
    maker: String,
    model: String,
    mount: String,
    crop: f32,
}
#[derive(Clone, Debug, Default)]
pub struct Database {
    pub profiles: Vec<Profile>,
    cameras: Vec<Camera>,
    mounts: std::collections::BTreeMap<String, Vec<String>>,
}
fn normalized(s: &str) -> String {
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn model_key(s: &str, maker: &str) -> String {
    let s = normalized(s);
    let maker = normalized(maker);
    s.strip_prefix(&maker)
        .unwrap_or(&s)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}
fn text(n: roxmltree::Node<'_, '_>, tag: &str) -> Option<String> {
    n.children()
        .find(|c| c.has_tag_name(tag) && c.attribute("lang").is_none())
        .and_then(|c| c.text())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}
fn number(s: &str) -> Option<f32> {
    s.parse::<f32>().ok().filter(|v| v.is_finite())
}
fn attr(n: roxmltree::Node<'_, '_>, key: &str, default: f32) -> Option<f32> {
    n.attribute(key)
        .map(number)
        .unwrap_or(Some(default))
        .filter(|v| v.abs() <= 20.0)
}
fn positive(n: roxmltree::Node<'_, '_>, tag: &str) -> Option<f32> {
    text(n, tag)
        .as_deref()
        .and_then(number)
        .filter(|v| *v >= 0.1 && *v <= 20.0)
}
fn stable_id(s: &str) -> u64 {
    // Two 24-bit words remain exactly representable in f32 recipe slots.
    (s.bytes().fold(14695981039346656037u64, |h, b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    }) & 0xffffffffffff)
        .max(1)
}
impl Database {
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty() && self.cameras.is_empty()
    }
    /// Reject invalid XML and oversized input. Unsupported models are omitted,
    /// never interpreted as a different polynomial. DTD/entity expansion is disabled.
    pub fn parse(xml: &str) -> Result<Self, &'static str> {
        if xml.len() > MAX_XML {
            return Err(t("lens_profiles.invalid"));
        }
        let doc = roxmltree::Document::parse(xml).map_err(|_| t("lens_profiles.invalid"))?;
        let root = doc.root_element();
        if !root.has_tag_name("lensdatabase")
            || !matches!(root.attribute("version"), Some("1" | "2"))
        {
            return Err(t("lens_profiles.invalid"));
        }
        let mut db = Self::default();
        for n in root.children().filter(|n| n.is_element()) {
            if n.has_tag_name("mount") {
                if let Some(name) = text(n, "name") {
                    let compatible = n
                        .children()
                        .filter(|c| c.has_tag_name("compat"))
                        .filter_map(|c| c.text().map(str::to_owned))
                        .collect();
                    db.mounts.insert(name, compatible);
                }
                continue;
            }
            let (Some(maker), Some(model)) = (text(n, "maker"), text(n, "model")) else {
                continue;
            };
            let Some(crop) = positive(n, "cropfactor") else {
                continue;
            };
            if n.has_tag_name("camera") {
                if let Some(mount) = text(n, "mount") {
                    db.cameras.push(Camera {
                        maker,
                        model,
                        mount,
                        crop,
                    });
                }
                continue;
            }
            if !n.has_tag_name("lens")
                || text(n, "type").is_some_and(|s| s != "rectilinear")
                || n.children().any(|c| c.has_tag_name("center"))
            {
                continue;
            }
            let aspect = match text(n, "aspect-ratio") {
                None => 1.5,
                Some(s) => match s.split_once(':') {
                    Some((a, b)) => number(a).zip(number(b)).map(|(a, b)| a / b),
                    None => number(&s),
                }
                .filter(|a| a.is_finite() && *a >= 1.0 && *a <= 4.0)
                .unwrap_or(0.0),
            };
            if aspect == 0.0 {
                continue;
            }
            let mounts: Vec<_> = n
                .children()
                .filter(|c| c.has_tag_name("mount"))
                .filter_map(|c| c.text().map(str::to_owned))
                .collect();
            let full_name = if normalized(&model).starts_with(&normalized(&maker)) {
                model.clone()
            } else {
                format!("{maker} {model}")
            };
            let name = format!("{full_name} ({crop}×, {aspect:.2}:1)");
            let identity = format!("{name}|{aspect}|{}", mounts.join("|"));
            let mut p = Profile {
                name,
                id: stable_id(&identity),
                maker,
                model,
                mounts,
                crop,
                aspect,
                distortion: Vec::new(),
                red: Vec::new(),
                blue: Vec::new(),
                vignette: Vec::new(),
            };
            for cal in n.children().filter(|c| c.has_tag_name("calibration")) {
                // v2 calibration-specific crop/aspect cannot silently use lens defaults.
                if cal.attributes().next().is_some() {
                    continue;
                }
                for c in cal.children().filter(|c| c.is_element()) {
                    let Some(focal) = c
                        .attribute("focal")
                        .and_then(number)
                        .filter(|v| *v > 0.0 && *v <= 10000.0)
                    else {
                        continue;
                    };
                    let get = |key, default| attr(c, key, default);
                    if c.has_tag_name("distortion") {
                        let values = match c.attribute("model") {
                            Some("poly3") => get("k1", 0.).map(|k| (1, [0., k, 0., 0.])),
                            Some("poly5") => get("k1", 0.)
                                .zip(get("k2", 0.))
                                .map(|(a, b)| (2, [a, b, 0., 0.])),
                            Some("ptlens") => get("a", 0.)
                                .zip(get("b", 0.))
                                .zip(get("c", 0.))
                                .map(|((a, b), c)| (3, [a, b, c, 0.])),
                            _ => None,
                        };
                        if let Some((model, coefficients)) = values {
                            p.distortion.push(Sample {
                                focal,
                                model,
                                coefficients,
                            });
                        }
                    } else if c.has_tag_name("vignetting") && c.attribute("model") == Some("pa") {
                        let aperture = c
                            .attribute("aperture")
                            .and_then(number)
                            .filter(|v| *v > 0. && *v <= 128.);
                        let distance = c
                            .attribute("distance")
                            .and_then(number)
                            .filter(|v| *v > 0. && *v <= 1000000.);
                        if let (Some(aperture), Some(distance), Some(k1), Some(k2), Some(k3)) = (
                            aperture,
                            distance,
                            get("k1", 0.),
                            get("k2", 0.),
                            get("k3", 0.),
                        ) {
                            p.vignette.push(Vignette {
                                focal,
                                aperture,
                                distance,
                                k: [k1, k2, k3],
                            });
                        }
                    } else if c.has_tag_name("tca") {
                        let mut samples = Vec::new();
                        for (b, cx, v, k) in [("br", "cr", "vr", "kr"), ("bb", "cb", "vb", "kb")] {
                            let coefficients = match c.attribute("model") {
                                Some("linear") => get(k, 1.).map(|v| [0., 0., v, 0.]),
                                Some("poly3") => get(b, 0.)
                                    .zip(get(cx, 0.))
                                    .zip(get(v, 1.))
                                    .map(|((b, c), v)| [b, c, v, 0.]),
                                _ => None,
                            };
                            if let Some(coefficients) = coefficients {
                                samples.push(Sample {
                                    focal,
                                    model: 1,
                                    coefficients,
                                });
                            }
                        }
                        if samples.len() == 2 {
                            p.blue.push(samples.pop().unwrap());
                            p.red.push(samples.pop().unwrap());
                        }
                    }
                }
            }
            if !p.distortion.is_empty() || !p.red.is_empty() || !p.vignette.is_empty() {
                db.profiles.push(p);
            }
        }
        Ok(db)
    }
    pub fn merge(&mut self, other: Self) {
        for p in other.profiles {
            if !self.profiles.iter().any(|q| q.id == p.id) {
                self.profiles.push(p);
            }
        }
        self.cameras.extend(other.cameras);
        self.mounts.extend(other.mounts);
        self.profiles.sort_by(|a, b| a.name.cmp(&b.name));
    }
    fn camera(&self, maker: &str, model: &str) -> Option<&Camera> {
        let cameras: Vec<_> = self
            .cameras
            .iter()
            .filter(|c| {
                normalized(&c.maker) == normalized(maker)
                    && model_key(&c.model, &c.maker) == model_key(model, maker)
            })
            .collect();
        let camera = *cameras.first()?;
        if cameras
            .iter()
            .any(|c| c.crop != camera.crop || c.mount != camera.mount)
        {
            return None;
        }
        Some(camera)
    }
    pub fn camera_crop(&self, maker: &str, model: &str) -> Option<f32> {
        self.camera(maker, model).map(|c| c.crop)
    }
    /// Only a unique, exact normalized camera/lens match can select automatically.
    /// Missing camera crop, mount mismatch, ambiguous identities or focal coverage
    /// leave correction disabled. No fuzzy guesses or extrapolation.
    pub fn matching(&self, maker: &str, model: &str, lens: &str, focal: f32) -> Option<(u64, f32)> {
        let camera = self.camera(maker, model)?;
        let found: Vec<_> = self
            .profiles
            .iter()
            .filter(|p| {
                let compatible = self.mounts.get(&camera.mount);
                model_key(&p.model, &p.maker) == model_key(lens, &p.maker)
                    && (p.mounts.contains(&camera.mount)
                        || compatible
                            .is_some_and(|mounts| p.mounts.iter().any(|m| mounts.contains(m))))
                    && camera.crop + 0.01 >= p.crop
                    && (p.coefficients(focal).is_some()
                        || (p.vignette.iter().any(|s| s.focal <= focal)
                            && p.vignette.iter().any(|s| s.focal >= focal)))
            })
            .collect();
        let best_crop = found.iter().map(|p| p.crop).reduce(f32::max)?;
        let found: Vec<_> = found
            .into_iter()
            .filter(|p| (p.crop - best_crop).abs() < 0.001)
            .collect();
        (found.len() == 1).then(|| (found[0].id, camera.crop))
    }
}
fn interpolate(samples: &[Sample], focal: f32) -> Option<(u8, [f32; 4])> {
    let low = samples
        .iter()
        .filter(|s| s.focal <= focal)
        .max_by(|a, b| a.focal.total_cmp(&b.focal))?;
    let high = samples
        .iter()
        .filter(|s| s.focal >= focal)
        .min_by(|a, b| a.focal.total_cmp(&b.focal))?;
    if low.model != high.model {
        return None;
    }
    let weight = if low.focal == high.focal {
        0.
    } else {
        (focal - low.focal) / (high.focal - low.focal)
    };
    Some((
        low.model,
        std::array::from_fn(|i| {
            low.coefficients[i] * (1. - weight) + high.coefficients[i] * weight
        }),
    ))
}
// Model, distortion terms, red TCA terms, blue TCA terms.
type GeometryCalibration = (u8, [f32; 4], [f32; 4], [f32; 4]);
impl Profile {
    fn coefficients(&self, focal: f32) -> Option<GeometryCalibration> {
        let distortion = interpolate(&self.distortion, focal);
        let tca = interpolate(&self.red, focal).zip(interpolate(&self.blue, focal));
        if distortion.is_none() && tca.is_none() {
            return None;
        }
        let (model, d) = distortion.unwrap_or((0, [0.; 4]));
        let (r, b) = tca
            .map(|((_, r), (_, b))| (r, b))
            .unwrap_or(([0., 0., 1., 0.], [0., 0., 1., 0.]));
        Some((model, d, r, b))
    }
    /// Bounded inverse-distance interpolation in focal length, aperture and
    /// reciprocal focus distance. Exact measured points are reproduced exactly.
    /// This is intentionally not Lensfun's spline interpolation.
    fn vignette_at(&self, focal: f32, aperture: f32, distance: f32) -> Option<[f32; 3]> {
        if !focal.is_finite()
            || !aperture.is_finite()
            || !distance.is_finite()
            || focal <= 0.
            || aperture <= 0.
            || distance <= 0.
        {
            return None;
        }
        let coords = |s: &Vignette| [s.focal, s.aperture, 1. / s.distance];
        let query = [focal, aperture, 1. / distance];
        let mut spans = [0.; 3];
        for axis in 0..3 {
            let lo = self
                .vignette
                .iter()
                .map(|s| coords(s)[axis])
                .reduce(f32::min)?;
            let hi = self
                .vignette
                .iter()
                .map(|s| coords(s)[axis])
                .reduce(f32::max)?;
            if query[axis] < lo - 1e-6 || query[axis] > hi + 1e-6 {
                return None;
            }
            spans[axis] = (hi - lo).max(1e-6);
        }
        let mut nearest: Vec<_> = self
            .vignette
            .iter()
            .map(|s| {
                let c = coords(s);
                (
                    (0..3)
                        .map(|i| ((query[i] - c[i]) / spans[i]).powi(2))
                        .sum::<f32>(),
                    s,
                )
            })
            .collect();
        nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
        if nearest[0].0 < 1e-12 {
            return Some(nearest[0].1.k);
        }
        let mut sum = [0.; 3];
        let mut weight = 0.;
        for (d, s) in nearest.into_iter().take(8) {
            let w = 1. / d;
            weight += w;
            for (sum, k) in sum.iter_mut().zip(s.k) {
                *sum += w * k;
            }
        }
        Some(sum.map(|k| k / weight))
    }
    /// Snapshot the selected calibration. It remains usable after database changes.
    pub fn bake(&self, focal: f32, crop: f32, values: &mut FilterValues) -> bool {
        let vignette =
            self.vignette_at(focal, values.get("lp_aperture"), values.get("lp_distance"));
        let Some((model, d, r, b)) = self
            .coefficients(focal)
            .or_else(|| vignette.map(|_| (0, [0.; 4], [0., 0., 1., 0.], [0., 0., 1., 0.])))
        else {
            return false;
        };
        if !crop.is_finite() || crop < self.crop - 0.01 || crop > 20. {
            return false;
        }
        for (key, value) in KEYS.into_iter().zip([
            1.,
            (self.id & 0xffffff) as f32,
            focal,
            crop,
            self.crop,
            self.aspect,
            model as f32,
            d[0],
            d[1],
            d[2],
            d[3],
            r[0],
            r[1],
            r[2],
            b[2],
        ]) {
            values.set(key, value);
        }
        values.set("lp_id_hi", (self.id >> 24) as f32);
        values.set("lp_bb", b[0]);
        values.set("lp_cb", b[1]);
        values.set("lp_vig_available", f32::from(vignette.is_some()));
        for (key, value) in ["lp_vk1", "lp_vk2", "lp_vk3"]
            .into_iter()
            .zip(vignette.unwrap_or([0.; 3]))
        {
            values.set(key, value);
        }

        true
    }
}
pub fn params() -> Vec<FilterParam> {
    KEYS.into_iter()
        .chain(EXTRA_KEYS)
        .map(|key| {
            let (min, max, default) = match key {
                "lp_enabled" | "lp_vignette" | "lp_vig_available" => (0., 1., 0.),
                "lp_aperture" => (0., 128., 0.),
                "lp_distance" => (0., 1000000., 0.),
                "lp_id" | "lp_id_hi" => (0., 16777215., 0.),
                "lp_focal" => (0.1, 10000., 50.),
                "lp_crop" | "lp_cal_crop" => (0.1, 20., 1.),
                "lp_aspect" => (1., 4., 1.5),
                "lp_model" => (0., 3., 0.),
                "lp_vr" | "lp_vb" => (-20., 20., 1.),
                _ => (-20., 20., 0.),
            };
            param(key, t("lens_profiles.calibration"), min, max, default, "")
        })
        .collect()
}
pub fn profile_id(v: &FilterValues) -> u64 {
    ((v.get("lp_id_hi") as u64) << 24) | v.get("lp_id") as u64
}
pub fn set_profile_id(v: &mut FilterValues, id: u64) {
    v.set("lp_id", (id & 0xffffff) as f32);
    v.set("lp_id_hi", (id >> 24) as f32);
}
pub fn enabled(v: &FilterValues) -> bool {
    v.get("lp_enabled") >= 0.5
}

/// Correct distortion and lateral CA in one bilinear resampling pass.
/// Coordinates follow Lensfun 0.3.x pixel-centre and calibration sensor conventions.
pub fn apply(px: &mut [f32], w: usize, h: usize, v: &FilterValues) {
    if !enabled(v) || w < 2 || h < 2 || px.len() != w.saturating_mul(h).saturating_mul(4) {
        return;
    }
    if params().iter().any(|p| {
        let x = v.get(p.key);
        !x.is_finite() || x < p.min || x > p.max
    }) {
        return;
    }
    let (cx, cy) = ((w - 1) as f32 * 0.5, (h - 1) as f32 * 0.5);
    let norm = v.get("lp_cal_crop") / v.get("lp_crop") * (1. + v.get("lp_aspect").powi(2)).sqrt()
        / cx.hypot(cy);
    let (a, b, c) = (v.get("lp_a"), v.get("lp_b"), v.get("lp_c"));
    let model = v.get("lp_model") as u8;
    let red = [v.get("lp_br"), v.get("lp_cr"), v.get("lp_vr")];
    let blue = [v.get("lp_bb"), v.get("lp_cb"), v.get("lp_vb")];
    let distortion = |r: f32| match model {
        1 => 1. - b + b * r * r,
        2 => 1. + a * r * r + b * r.powi(4),
        3 => a * r.powi(3) + b * r * r + c * r + 1. - a - b - c,
        _ => 1.,
    };
    if v.get("lp_vignette") >= 0.5 && v.get("lp_vig_available") >= 0.5 {
        let radial_scale = (v.get("lp_cal_crop") / v.get("lp_crop")).powi(2) / (cx * cx + cy * cy);
        let k = [v.get("lp_vk1"), v.get("lp_vk2"), v.get("lp_vk3")];
        for y in 0..h {
            for x in 0..w {
                let r2 = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)) * radial_scale;
                let attenuation = 1. + k[0] * r2 + k[1] * r2 * r2 + k[2] * r2 * r2 * r2;
                // Invalid/near-singular profiles never generate unbounded highlights.
                let gain = if attenuation.is_finite() && attenuation > 0.0625 {
                    (1. / attenuation).min(16.)
                } else {
                    1.
                };
                if gain != 1. {
                    for channel in &mut px[(y * w + x) * 4..(y * w + x) * 4 + 3] {
                        *channel = encode_srgb(decode_srgb(*channel) * gain);
                    }
                }
            }
        }
    }
    if (v.get("lp_model") == 0. || (a == 0. && b == 0. && c == 0.))
        && ["lp_br", "lp_cr", "lp_bb", "lp_cb"]
            .iter()
            .all(|k| v.get(k) == 0.)
        && v.get("lp_vr") == 1.
        && v.get("lp_vb") == 1.
    {
        return;
    }
    premultiply(px);
    let src = px.to_vec();
    for y in 0..h {
        for x in 0..w {
            let (u, vv) = ((x as f32 - cx) * norm, (y as f32 - cy) * norm);
            let d = distortion(u.hypot(vv));
            let (u, vv) = (u * d, vv * d);
            let r = u.hypot(vv);
            let scales = [
                red[0] * r * r + red[1] * r + red[2],
                1.,
                blue[0] * r * r + blue[1] * r + blue[2],
            ];
            let i = (y * w + x) * 4;
            for (channel, scale) in scales.into_iter().enumerate() {
                let (sx, sy) = (cx + u * scale / norm, cy + vv * scale / norm);
                let pixel = if sx.is_finite()
                    && sy.is_finite()
                    && sx >= 0.
                    && sy >= 0.
                    && sx <= (w - 1) as f32
                    && sy <= (h - 1) as f32
                {
                    sample(&src, w, h, sx, sy)
                } else {
                    [0.; 4]
                };
                px[i + channel] = pixel[channel];
                if channel == 1 {
                    px[i + 3] = pixel[3];
                }
            }
        }
    }
    unpremultiply(px);
}

fn decode_srgb(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn encode_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}

/// Strict recognition of Schist's built-in sRGB encoding. Timestamps and
/// profile IDs do not change colorimetry; every other ICC byte must match.
/// Unknown/external variants are conservatively refused, never guessed by name.
pub fn supported_srgb(icc: Option<&[u8]>) -> bool {
    static SRGB: OnceLock<Vec<u8>> = OnceLock::new();
    let Some(icc) = icc else { return false };
    let expected = SRGB.get_or_init(|| {
        schist_colormgmt::Profile::srgb()
            .icc_bytes()
            .unwrap_or_default()
            .to_vec()
    });
    !expected.is_empty()
        && icc.len() == expected.len()
        && icc
            .iter()
            .zip(expected)
            .enumerate()
            .all(|(i, (a, b))| (24..36).contains(&i) || (84..100).contains(&i) || a == b)
}

static DATABASE: OnceLock<RwLock<Database>> = OnceLock::new();
pub fn database() -> &'static RwLock<Database> {
    DATABASE.get_or_init(|| RwLock::new(Database::default()))
}
#[cfg(not(target_arch = "wasm32"))]
pub fn load_installed_profiles() {
    static LOAD: std::sync::Once = std::sync::Once::new();
    LOAD.call_once(|| {
        let installed = load_installed();
        database()
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .merge(installed);
    });
}
#[cfg(not(target_arch = "wasm32"))]
fn load_installed() -> Database {
    let mut db = Database::default();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut dirs = Vec::new();
        if let Some(dir) = std::env::var_os("SCHIST_LENSFUN_DIR") {
            dirs.push(std::path::PathBuf::from(dir));
        }
        // User directories take precedence over system profiles with the same identity.
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(
                std::path::PathBuf::from(home).join(".local/share/lensfun/updates/version_1"),
            );
        }
        dirs.extend(
            [
                "/usr/share/lensfun/version_1",
                "/usr/share/lensfun",
                "/usr/local/share/lensfun/version_1",
                "/opt/homebrew/share/lensfun/version_1",
            ]
            .map(Into::into),
        );
        let mut remaining = MAX_DATABASE;
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut paths: Vec<_> = entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|s| s == "xml"))
                .take(4096)
                .collect();
            paths.sort();
            for path in paths {
                let Ok(meta) = path.metadata() else { continue };
                let len = meta.len() as usize;
                if !meta.is_file() || len > MAX_XML || len > remaining {
                    continue;
                }
                remaining -= len;
                if let Ok(file) = std::fs::File::open(path) {
                    use std::io::Read;
                    let mut xml = String::new();
                    if file
                        .take(MAX_XML as u64 + 1)
                        .read_to_string(&mut xml)
                        .is_err()
                    {
                        continue;
                    }
                    match Database::parse(&xml) {
                        Ok(parsed) => db.merge(parsed),
                        Err(e) => log::warn!("Lensfun database: {e}"),
                    }
                }
            }
        }
    }
    db
}

#[cfg(test)]
mod tests {
    use super::*;
    use schist_plugin_api::FilterPlugin;
    const MEASURED: &str = include_str!("../tests/fixtures/lensfun-canon.xml");
    fn measured() -> (Database, FilterValues) {
        let db = Database::parse(MEASURED).unwrap();
        let mut v = FilterValues::defaults(&crate::lens::LensCorrection.params());
        v.set("lp_aperture", 2.8);
        v.set("lp_distance", 1000.);
        assert!(db.profiles[0].bake(40., 1., &mut v));
        (db, v)
    }
    #[test]
    fn lens_profiles_real_camera_match_is_conservative() {
        let (db, v) = measured();
        assert_eq!(
            db.matching("Canon", "Canon EOS 5D Mark II", "EF40mm f/2.8 STM", 40.),
            Some((profile_id(&v), 1.))
        );
        assert!(db
            .matching("Canon", "Unknown camera", "EF40mm f/2.8 STM", 40.)
            .is_none());
        assert!(db
            .matching("Canon", "Canon EOS 5D Mark II", "EF40mm f/2.8 STM", 41.)
            .is_none());
        assert!(db
            .matching("Canon", "Canon EOS 5D Mark II", "EF40mm f/2.8", 40.)
            .is_none());
        let mut ambiguous = db.clone();
        let mut duplicate = db.profiles[0].clone();
        duplicate.id += 1;
        ambiguous.profiles.push(duplicate);
        assert!(ambiguous
            .matching("Canon", "Canon EOS 5D Mark II", "EF40mm f/2.8 STM", 40.)
            .is_none());
    }
    #[test]
    fn lens_profiles_measured_coefficients_and_focal_bounds() {
        let (db, v) = measured();
        let p = &db.profiles[0];
        assert_eq!(v.get("lp_a"), 0.01087);
        assert_eq!(v.get("lp_b"), -0.02951);
        assert_eq!(v.get("lp_vr"), 1.0003216);
        assert_eq!(v.get("lp_vk1"), -1.5776);
        assert_eq!(v.get("lp_vig_available"), 1.);
        assert!(p.vignette_at(40., 2.8, 1000.).is_some());
        assert!(p.vignette_at(40., 2.8, 0.).is_none());
        assert!(p.vignette_at(40., 1.4, 1000.).is_none());
        assert!(p.vignette_at(40., 2.8, 2000.).is_none());
        let k = p.vignette_at(40., 3.2, 500.).unwrap();
        assert!(k.iter().all(|x| x.is_finite()));
        assert!(k[0] >= -1.5776 && k[0] <= -0.3194);
    }
    #[test]
    fn lens_profiles_interpolation_preserves_endpoints_and_rejects_model_changes() {
        // Synthetic polynomial arithmetic, deliberately not a claimed lens calibration.
        let a = Sample {
            focal: 20.,
            model: 1,
            coefficients: [0., -0.02, 0., 0.],
        };
        let b = Sample {
            focal: 40.,
            model: 1,
            coefficients: [0., 0.02, 0., 0.],
        };
        assert_eq!(
            interpolate(&[a.clone(), b.clone()], 20.).unwrap().1,
            a.coefficients
        );
        assert_eq!(
            interpolate(&[a.clone(), b.clone()], 30.).unwrap().1,
            [0.; 4]
        );
        assert!(interpolate(&[a.clone(), b.clone()], 50.).is_none());
        let mut different = b;
        different.model = 3;
        assert!(interpolate(&[a, different], 30.).is_none());
    }
    #[test]
    fn lens_profiles_parser_rejects_untrusted_and_unsupported_data() {
        assert!(
            Database::parse("<!DOCTYPE a [<!ENTITY b 'x'>]><lensdatabase version='1'/>").is_err()
        );
        assert!(Database::parse("<lensdatabase version='99'/>").is_err());
        assert!(Database::parse(&"x".repeat(MAX_XML + 1)).is_err());
        let xml = MEASURED.replace("<cropfactor>1</cropfactor>", "<cropfactor>NaN</cropfactor>");
        assert!(Database::parse(&xml).unwrap().profiles.is_empty());
        let xml = MEASURED.replace("<calibration>", "<type>fisheye</type><calibration>");
        assert!(Database::parse(&xml).unwrap().profiles.is_empty());
        let xml = MEASURED.replace("model=\"ptlens\"", "model=\"acm\"");
        let p = Database::parse(&xml).unwrap().profiles.remove(0);
        assert_eq!(p.coefficients(40.).unwrap().0, 0);
    }
    #[test]
    fn lens_profiles_neutral_is_bit_exact_and_nonfinite_is_rejected() {
        let (_, mut v) = measured();
        for k in ["lp_a", "lp_b", "lp_c", "lp_br", "lp_cr", "lp_bb", "lp_cb"] {
            v.set(k, 0.);
        }
        v.set("lp_vr", 1.);
        v.set("lp_vb", 1.);
        let src = vec![
            0.7, 0.1, 0.4, 0., 0.2, 0.3, 0.4, 0.5, 0.1, 0.2, 0.3, 1., 0.9, 0.8, 0.7, 1.,
        ];
        let mut actual = src.clone();
        apply(&mut actual, 2, 2, &v);
        assert_eq!(actual, src);
        v.set("lp_a", f32::NAN);
        apply(&mut actual, 2, 2, &v);
        assert_eq!(actual, src);
    }
    #[test]
    fn lens_profiles_pa_uses_linear_light_before_geometry() {
        let (_, mut v) = measured();
        // Isolate the real profile's measured vignetting from its geometry/TCA.
        v.set("lp_model", 0.);
        for k in ["lp_br", "lp_cr", "lp_bb", "lp_cb"] {
            v.set(k, 0.);
        }
        v.set("lp_vr", 1.);
        v.set("lp_vb", 1.);
        v.set("lp_vignette", 1.);
        let mut image = [0.25, 0.25, 0.25, 0.4].repeat(25);
        apply(&mut image, 5, 5, &v);
        let expected = encode_srgb(decode_srgb(0.25) / (1. - 1.5776 + 1.3808 - 0.5311));
        assert!((image[0] - expected).abs() < 1e-5);
        assert!((image[12 * 4] - 0.25).abs() < 1e-6);
        assert!(image.as_chunks::<4>().0.iter().all(|p| p[3] == 0.4));
        // A malformed imported polynomial must never cause infinite amplification.
        v.set("lp_vk1", -20.);
        apply(&mut image, 5, 5, &v);
        assert!(image.iter().all(|p| p.is_finite()));
    }
    #[test]
    fn lens_profiles_geometry_matches_analytic_source_coordinate() {
        let (_, mut v) = measured();
        v.set("lp_br", 0.);
        v.set("lp_cr", 0.);
        v.set("lp_bb", 0.);
        v.set("lp_cb", 0.);
        v.set("lp_vr", 1.);
        v.set("lp_vb", 1.);
        let (w, h) = (101, 67);
        let mut image: Vec<_> = (0..h)
            .flat_map(|_| (0..w).flat_map(|x| [x as f32 / 100., 0., 0., 1.]))
            .collect();
        apply(&mut image, w, h, &v);
        let norm = 3.25f32.sqrt() / 50f32.hypot(33.);
        let r = 20. * norm;
        let k =
            0.01087 * r.powi(3) - 0.02951 * r * r + 0.0162 * r + 1. - 0.01087 + 0.02951 - 0.0162;
        let expected = (50. + 20. * k) / 100.;
        assert!((image[(33 * w + 70) * 4] - expected).abs() < 1e-5);
    }
    #[test]
    fn lens_profiles_tca_samples_each_channel_after_distortion() {
        let (_, v) = measured();
        let (w, h) = (101, 67);
        let mut image: Vec<_> = (0..h)
            .flat_map(|_| (0..w).flat_map(|x| [x as f32 / 100., 0.4, x as f32 / 100., 1.]))
            .collect();
        apply(&mut image, w, h, &v);
        let r = 20. * 3.25f32.sqrt() / 50f32.hypot(33.);
        let k =
            0.01087 * r.powi(3) - 0.02951 * r * r + 0.0162 * r + 1. - 0.01087 + 0.02951 - 0.0162;
        let rd = r * k;
        let red = (50. + 20. * k * (-0.0001009 * rd * rd + 1.0003216)) / 100.;
        let blue = (50. + 20. * k * (0.0001426 * rd * rd + 0.9997719)) / 100.;
        let p = &image[(33 * w + 70) * 4..(33 * w + 70) * 4 + 4];
        assert!((p[0] - red).abs() < 1e-5);
        assert!((p[2] - blue).abs() < 1e-5);
        assert!((p[1] - 0.4).abs() < 1e-6);
        assert_eq!(p[3], 1.);
    }
    #[test]
    fn lens_profiles_color_context_requires_explicit_builtin_srgb() {
        let srgb = schist_colormgmt::Profile::srgb();
        assert!(supported_srgb(srgb.icc_bytes()));
        assert!(!supported_srgb(None));
        assert!(!supported_srgb(Some(&[0; 128])));
        assert!(!supported_srgb(
            schist_colormgmt::Profile::display_p3().icc_bytes()
        ));
        let mut dated = srgb.icc_bytes().unwrap().to_vec();
        dated[24] ^= 1;
        dated[84] ^= 1;
        assert!(supported_srgb(Some(&dated)));
        *dated.last_mut().unwrap() ^= 1;
        assert!(!supported_srgb(Some(&dated)));
    }
    #[test]
    fn lens_profiles_recipe_replays_without_database_and_disables_wrong_color_context() {
        use schist_core::filter_stack::FilterEffect;
        let (_, mut v) = measured();
        v.set("lp_vignette", 1.);
        let effect = FilterEffect {
            id: "filter.lens_correction".into(),
            enabled: true,
            values: v.0.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            foreground: [0., 0., 0., 1.],
            background: [1.; 4],
        };
        let filter = crate::lens::LensCorrection;
        let mut stack =
            schist_core::filter_stack::FilterStack::new(schist_core::IntRect::new(0, 0, 11, 9));
        stack.effects.push(effect);
        let mut layer = schist_core::Layer::new_raster("profile persistence test");
        layer.extras = stack
            .blocks(&layer, &schist_core::TileMap::default())
            .unwrap();
        let reopened = schist_core::filter_stack::FilterStack::read(&layer)
            .unwrap()
            .unwrap();
        assert_eq!(reopened, stack);
        let restored =
            schist_plugin_api::filter_stack::values(&filter, &reopened.effects[0]).unwrap();
        assert_eq!(restored, v);
        assert!(filter.gpu_operation(&v).is_none());
        let mut a = [0.25, 0.3, 0.4, 1.].repeat(99);
        let mut b = a.clone();
        filter.apply(&mut a, 11, 9, &v);
        filter.apply(&mut b, 11, 9, &restored);
        assert_eq!(a, b);
        let mut native = schist_plugin_api::NativeFilterBuffer {
            mode: schist_color::ColorMode::Rgb,
            width: 11,
            height: 9,
            pixels: (0..99)
                .map(|_| {
                    schist_color::NativePixel::from_rgba(
                        schist_color::ColorMode::Rgb,
                        schist_color::Rgba::new(0.25, 0.3, 0.4, 1.),
                    )
                })
                .collect(),
            icc_profile: Some(vec![0]),
        };
        let mut no_vig = native.clone();
        let mut v2 = v.clone();
        v2.set("lp_vignette", 0.);
        filter.apply_native_with(&mut native, &v, &Default::default());
        filter.apply_native_with(&mut no_vig, &v2, &Default::default());
        assert_eq!(native.pixels, no_vig.pixels);
    }
}
