//! Scene-linear RGB profiles described by CIE xy chromaticities.
//!
//! OpenEXR stores scene-linear light with an optional `chromaticities`
//! attribute naming the primaries and white point (Rec. 709 / D65 when
//! absent). Schist documents carry their colour meaning as an ICC
//! profile, so an EXR's chromaticities become a matrix/shaper profile
//! with identity tone curves, and the reverse direction recovers the
//! chromaticities from a document profile on export.

use anyhow::{anyhow, Result};
use moxcms::{
    Chromaticity, ColorPrimaries, ColorProfile, LocalizableString, ProfileText, ToneReprCurve, Xyzd,
};

/// CIE 1931 xy coordinates of an RGB space's primaries and white point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chromaticities {
    pub red: [f32; 2],
    pub green: [f32; 2],
    pub blue: [f32; 2],
    pub white: [f32; 2],
}

impl Chromaticities {
    /// ITU-R BT.709 / sRGB primaries, D65 white. The OpenEXR default.
    pub const REC709: Chromaticities = Chromaticities {
        red: [0.64, 0.33],
        green: [0.30, 0.60],
        blue: [0.15, 0.06],
        white: [0.3127, 0.3290],
    };
    /// Display P3 (P3 primaries, D65 white).
    pub const P3_D65: Chromaticities = Chromaticities {
        red: [0.680, 0.320],
        green: [0.265, 0.690],
        blue: [0.150, 0.060],
        white: [0.3127, 0.3290],
    };
    /// ITU-R BT.2020 primaries, D65 white.
    pub const REC2020: Chromaticities = Chromaticities {
        red: [0.708, 0.292],
        green: [0.170, 0.797],
        blue: [0.131, 0.046],
        white: [0.3127, 0.3290],
    };
    /// ACES AP0 (ACES2065-1), ACES white.
    pub const ACES_AP0: Chromaticities = Chromaticities {
        red: [0.7347, 0.2653],
        green: [0.0, 1.0],
        blue: [0.0001, -0.0770],
        white: [0.32168, 0.33767],
    };
    /// ACES AP1 (ACEScg), ACES white.
    pub const ACES_AP1: Chromaticities = Chromaticities {
        red: [0.713, 0.293],
        green: [0.165, 0.830],
        blue: [0.128, 0.044],
        white: [0.32168, 0.33767],
    };

    const KNOWN: [Chromaticities; 5] = [
        Self::REC709,
        Self::P3_D65,
        Self::REC2020,
        Self::ACES_AP0,
        Self::ACES_AP1,
    ];

    fn approx_eq(&self, other: &Chromaticities, tolerance: f32) -> bool {
        let a = [self.red, self.green, self.blue, self.white];
        let b = [other.red, other.green, other.blue, other.white];
        a.iter()
            .flatten()
            .zip(b.iter().flatten())
            .all(|(x, y)| (x - y).abs() <= tolerance)
    }

    /// Snap to a standard set when within rounding of it, so values that
    /// went through `f32` and back compare equal to their source.
    fn snapped(self) -> Chromaticities {
        Self::KNOWN
            .into_iter()
            .find(|known| self.approx_eq(known, 2e-3))
            .unwrap_or(self)
    }

    fn validate(&self) -> Result<()> {
        let points = [self.red, self.green, self.blue, self.white];
        if points.iter().flatten().any(|v| !v.is_finite()) || points.iter().any(|p| p[1] == 0.0) {
            return Err(anyhow!("invalid chromaticities {self:?}"));
        }
        // The primaries must span a triangle, or the RGB→XYZ matrix is singular.
        let [r, g, b] = [self.red, self.green, self.blue];
        let area = (g[0] - r[0]) * (b[1] - r[1]) - (b[0] - r[0]) * (g[1] - r[1]);
        if area.abs() < 1e-6 {
            return Err(anyhow!("degenerate chromaticities {self:?}"));
        }
        Ok(())
    }

    /// The text embedded in a profile's description, which lets
    /// [`linear_chromaticities`] return exactly what [`linear_profile`]
    /// was given rather than a value recovered through D50 adaptation.
    fn description(&self) -> String {
        let p = |v: [f32; 2]| format!("{} {}", v[0], v[1]);
        format!(
            "Scene-linear RGB (r {}, g {}, b {}, w {})",
            p(self.red),
            p(self.green),
            p(self.blue),
            p(self.white)
        )
    }

    fn from_description(text: &str) -> Option<Chromaticities> {
        let inner = text.strip_prefix("Scene-linear RGB (")?.strip_suffix(')')?;
        let mut out = [[0.0f32; 2]; 4];
        for (slot, (part, tag)) in out
            .iter_mut()
            .zip(inner.split(", ").zip(["r ", "g ", "b ", "w "]))
        {
            let mut numbers = part.strip_prefix(tag)?.split(' ');
            *slot = [numbers.next()?.parse().ok()?, numbers.next()?.parse().ok()?];
            if numbers.next().is_some() {
                return None;
            }
        }
        let [red, green, blue, white] = out;
        Some(Chromaticities {
            red,
            green,
            blue,
            white,
        })
    }
}

/// An ICC profile for scene-linear RGB with these chromaticities.
pub fn linear_profile(chroma: &Chromaticities) -> Result<Vec<u8>> {
    chroma.validate()?;
    let mut profile = ColorProfile::new_srgb();
    profile.cicp = None;
    let xy = |v: [f32; 2]| Chromaticity::new(v[0], v[1]);
    let white = xy(chroma.white);
    profile.update_rgb_colorimetry(
        white.to_xyyb(),
        ColorPrimaries {
            red: xy(chroma.red),
            green: xy(chroma.green),
            blue: xy(chroma.blue),
        },
    );
    profile.media_white_point = Some(white.to_xyzd());
    // An empty curve is the identity in ICC.
    let linear = ToneReprCurve::Lut(Vec::new());
    profile.red_trc = Some(linear.clone());
    profile.green_trc = Some(linear.clone());
    profile.blue_trc = Some(linear);
    profile.description = Some(ProfileText::Localizable(vec![LocalizableString::new(
        "en".to_string(),
        "US".to_string(),
        chroma.description(),
    )]));
    profile.copyright = None;
    profile
        .encode()
        .map_err(|e| anyhow!("cannot encode linear profile: {e:?}"))
}

fn is_linear_curve(curve: &Option<ToneReprCurve>) -> bool {
    match curve {
        Some(ToneReprCurve::Lut(lut)) => match lut.as_slice() {
            [] => true,
            // A single entry is a gamma in u8.8 fixed point.
            [gamma] => *gamma == 0x0100,
            _ => false,
        },
        Some(ToneReprCurve::Parametric(params)) => {
            // Type 0 is `Y = X^g`; the others reduce to it with a = 1, b = 0
            // and, for types 3 and 4, a linear segment that is the identity.
            let close = |v: f32, want: f32| (v - want).abs() < 1e-4;
            match params.as_slice() {
                [g] => close(*g, 1.0),
                [g, a, b, ..] => close(*g, 1.0) && close(*a, 1.0) && close(*b, 0.0),
                _ => false,
            }
        }
        None => false,
    }
}

fn description_text(profile: &ColorProfile) -> Option<&str> {
    match profile.description.as_ref()? {
        ProfileText::PlainString(s) => Some(s),
        ProfileText::Localizable(list) => list.first().map(|s| s.value.as_str()),
        ProfileText::Description(d) => Some(&d.ascii_string),
    }
}

/// Bradford cone response matrix.
const BRADFORD: [[f64; 3]; 3] = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

fn mul(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

fn inverse(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let c =
        |r0: usize, c0: usize, r1: usize, c1: usize| m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
    Some([
        [
            c(1, 1, 2, 2) / det,
            -c(0, 1, 2, 2) / det,
            c(0, 1, 1, 2) / det,
        ],
        [
            -c(1, 0, 2, 2) / det,
            c(0, 0, 2, 2) / det,
            -c(0, 0, 1, 2) / det,
        ],
        [
            c(1, 0, 2, 1) / det,
            -c(0, 0, 2, 1) / det,
            c(0, 0, 1, 1) / det,
        ],
    ])
}

/// Adapt an XYZ colour from one white to another (Bradford).
fn adapt(xyz: [f64; 3], from: [f64; 3], to: [f64; 3]) -> Option<[f64; 3]> {
    let inv = inverse(&BRADFORD)?;
    let (s, d) = (mul(&BRADFORD, from), mul(&BRADFORD, to));
    let cone = mul(&BRADFORD, xyz);
    let scaled = [0, 1, 2].map(|i| cone[i] * d[i] / s[i]);
    Some(mul(&inv, scaled))
}

fn xy_of(xyz: [f64; 3]) -> Option<[f32; 2]> {
    let sum = xyz[0] + xyz[1] + xyz[2];
    (sum.abs() > 1e-9).then(|| [(xyz[0] / sum) as f32, (xyz[1] / sum) as f32])
}

/// What an RGB matrix/shaper profile says about its primaries, and whether
/// its tone curves are linear. `None` for a profile that is not RGB
/// matrix/shaper (a LUT-based or CMYK profile) or cannot be read.
pub fn profile_chromaticities(icc: &[u8]) -> Option<(Chromaticities, bool)> {
    let profile = ColorProfile::new_from_slice(icc).ok()?;
    if profile.color_space != moxcms::DataColorSpace::Rgb {
        return None;
    }
    let linear = [&profile.red_trc, &profile.green_trc, &profile.blue_trc]
        .into_iter()
        .all(is_linear_curve);
    if let Some(chroma) = description_text(&profile).and_then(Chromaticities::from_description) {
        return Some((chroma, linear));
    }
    // The colorants are adapted to D50; undo that for the media white the
    // profile declares (D65 when it says nothing, as for most RGB spaces).
    let as_vec = |v: Xyzd| [v.x, v.y, v.z];
    let d50 = [0.9642, 1.0, 0.8249];
    let white = profile
        .media_white_point
        .map(as_vec)
        .filter(|w| w[1] > 0.0)
        .unwrap_or([0.95047, 1.0, 1.08883]);
    let primary = |v: Xyzd| xy_of(adapt(as_vec(v), d50, white)?);
    let chroma = Chromaticities {
        red: primary(profile.red_colorant)?,
        green: primary(profile.green_colorant)?,
        blue: primary(profile.blue_colorant)?,
        white: xy_of(white)?,
    };
    chroma.validate().ok()?;
    Some((chroma.snapped(), linear))
}

fn is_srgb_curve(curve: &Option<ToneReprCurve>) -> bool {
    let srgb = [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045];
    matches!(curve, Some(ToneReprCurve::Parametric(p))
        if p.len() == srgb.len() && p.iter().zip(srgb).all(|(a, b)| (a - b).abs() < 1e-3))
}

/// The sRGB EOTF extended past 0..1: mirrored below zero and continued
/// above one, so the HDR values a 32-bit document holds survive.
fn srgb_to_linear_extended(v: f32) -> f32 {
    let a = v.abs();
    let l = if a <= 0.04045 {
        a / 12.92
    } else {
        ((a + 0.055) / 1.055).powf(2.4)
    };
    l.copysign(v)
}

/// Convert straight-alpha RGBA pixels described by `icc` (sRGB when
/// `None`) to scene-linear light with the same primaries, and return
/// those primaries.
///
/// A document already in a linear RGB profile is returned untouched. The
/// sRGB curve is undone analytically rather than through the CMM, which
/// would clip the values above 1.0 that 32-bit documents (HDR merges
/// among them) store. Any other profile goes through the CMM to linear
/// light with its own primaries, or to linear Rec. 709 when its primaries
/// are not a matrix (LUT-based profiles).
pub fn to_scene_linear(pixels: &mut [f32], icc: Option<&[u8]>) -> Result<Chromaticities> {
    let Some(bytes) = icc else {
        linearise_srgb(pixels);
        return Ok(Chromaticities::REC709);
    };
    let source = crate::Profile::from_bytes(bytes)?;
    let (chroma, linear) = profile_chromaticities(bytes).unwrap_or((Chromaticities::REC709, false));
    if linear {
        return Ok(chroma);
    }
    let curves = [
        &source.profile.red_trc,
        &source.profile.green_trc,
        &source.profile.blue_trc,
    ];
    if profile_chromaticities(bytes).is_some() && curves.into_iter().all(is_srgb_curve) {
        linearise_srgb(pixels);
        return Ok(chroma);
    }
    let target = crate::Profile::from_bytes(&linear_profile(&chroma)?)?;
    crate::convert_pixels(
        pixels,
        &source,
        &target,
        crate::Intent::RelativeColorimetric,
    )?;
    Ok(chroma)
}

/// Tone-map scene-linear straight-alpha RGBA, described by the linear
/// profile `icc` (linear Rec. 709 when `None`), to display sRGB.
///
/// For previews of HDR content such as gallery thumbnails: 1.0 is taken
/// as diffuse white, brighter values roll off through
/// [`crate::highlight_shoulder`] instead of clipping, and negative or
/// non-finite samples become 0.
pub fn tone_map_linear_to_srgb(pixels: &mut [f32], icc: Option<&[u8]>) -> Result<()> {
    for px in pixels.as_chunks_mut::<4>().0 {
        for c in px.iter_mut().take(3) {
            let v = if c.is_finite() { c.max(0.0) } else { 0.0 };
            *c = crate::highlight_shoulder(v);
        }
        if !px[3].is_finite() {
            px[3] = 0.0;
        }
    }
    let source = match icc {
        Some(bytes) => crate::Profile::from_bytes(bytes)?,
        None => crate::Profile::from_bytes(&linear_profile(&Chromaticities::REC709)?)?,
    };
    crate::convert_pixels(
        pixels,
        &source,
        &crate::Profile::srgb(),
        crate::Intent::RelativeColorimetric,
    )
}

fn linearise_srgb(pixels: &mut [f32]) {
    for px in pixels.as_chunks_mut::<4>().0 {
        for c in px.iter_mut().take(3) {
            *c = srgb_to_linear_extended(*c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_profile_round_trips_its_chromaticities() {
        let odd = Chromaticities {
            red: [0.69, 0.3],
            green: [0.21, 0.71],
            blue: [0.149, 0.051],
            white: [0.3457, 0.3585],
        };
        for chroma in [
            Chromaticities::REC709,
            Chromaticities::ACES_AP0,
            Chromaticities::ACES_AP1,
            odd,
        ] {
            let icc = linear_profile(&chroma).unwrap();
            let (back, linear) = profile_chromaticities(&icc).unwrap();
            assert!(linear, "identity curves read as linear");
            assert_eq!(back, chroma);
        }
    }

    #[test]
    fn standard_profiles_report_their_primaries() {
        let srgb = moxcms::ColorProfile::new_srgb().encode().unwrap();
        let (chroma, linear) = profile_chromaticities(&srgb).unwrap();
        assert!(!linear, "sRGB has a non-linear curve");
        assert_eq!(chroma, Chromaticities::REC709);

        let p3 = moxcms::ColorProfile::new_display_p3().encode().unwrap();
        let (chroma, _) = profile_chromaticities(&p3).unwrap();
        assert_eq!(chroma, Chromaticities::P3_D65);
    }

    #[test]
    fn srgb_pixels_linearise() {
        let mut px = vec![0.5f32, 0.5, 0.5, 0.25, 1.0, 0.0, 0.0, 1.0];
        let chroma = to_scene_linear(&mut px, None).unwrap();
        assert_eq!(chroma, Chromaticities::REC709);
        // sRGB 0.5 is 0.214 linear.
        assert!((px[0] - 0.214).abs() < 2e-3, "{}", px[0]);
        assert_eq!(px[3], 0.25, "alpha untouched");
        assert!((px[4] - 1.0).abs() < 1e-3 && px[5].abs() < 1e-3);

        // Above-white values from an sRGB-tagged HDR merge are not clipped.
        let srgb = moxcms::ColorProfile::new_srgb().encode().unwrap();
        let mut hdr = vec![2.0f32, 1.0, 0.0, 1.0];
        assert_eq!(
            to_scene_linear(&mut hdr, Some(&srgb)).unwrap(),
            Chromaticities::REC709
        );
        assert!(hdr[0] > 4.0, "{}", hdr[0]);
        assert!((hdr[1] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn tone_mapping_keeps_white_and_rolls_off_highlights() {
        let mut px = vec![
            0.0f32,
            0.0,
            0.0,
            1.0, // black
            0.18,
            0.18,
            0.18,
            1.0, // mid grey
            1.0,
            1.0,
            1.0,
            1.0, // diffuse white
            16.0,
            16.0,
            16.0,
            1.0, // specular
            f32::NAN,
            -1.0,
            f32::INFINITY,
            0.5,
        ];
        tone_map_linear_to_srgb(&mut px, None).unwrap();
        assert!(px[0] < 0.01);
        assert!(
            (px[4] - 0.46).abs() < 0.02,
            "mid grey encodes as sRGB: {}",
            px[4]
        );
        assert!(
            px[8] > 0.95 && px[8] <= 1.0001,
            "white stays white: {}",
            px[8]
        );
        assert!(
            px[12] >= px[8] && px[12] <= 1.0001,
            "speculars do not clip past white"
        );
        assert!(px[16..19].iter().all(|v| v.is_finite() && *v >= 0.0));
        assert_eq!(px[19], 0.5);
    }

    #[test]
    fn a_linear_document_is_left_alone() {
        let icc = linear_profile(&Chromaticities::REC2020).unwrap();
        let mut px = vec![4.0f32, 0.5, 0.25, 1.0];
        let chroma = to_scene_linear(&mut px, Some(&icc)).unwrap();
        assert_eq!(chroma, Chromaticities::REC2020);
        assert_eq!(px, vec![4.0, 0.5, 0.25, 1.0], "HDR values survive");
    }
}
