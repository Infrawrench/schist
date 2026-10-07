//! Object effects: how an item blends with what lies beneath it, and the
//! drop shadow it casts.
//!
//! InDesign's PDF of the public paged-media `effects` sample draws a drop
//! shadow as the effect colour painted at the shadow's opacity and blend
//! mode through a soft mask: the item's shape moved by the offsets (right and
//! down) and blurred. Across shadows of size 6 and 24 pt the mask's edge is
//! the same curve scaled by the size, a Gaussian of deviation half the size
//! to within 1.2 %, its tails cut about 1.2 sizes from the edge.
use serde::{Deserialize, Serialize};

/// How colour is combined with what lies beneath it, as the specification's
/// BlendMode names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    SoftLight,
    HardLight,
    ColorDodge,
    ColorBurn,
    Darken,
    Lighten,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    /// Whether each channel blends on its own. The others mix hue,
    /// saturation and luminosity across channels; Schist draws them Normal.
    pub fn separable(self) -> bool {
        !matches!(
            self,
            Self::Hue | Self::Saturation | Self::Color | Self::Luminosity
        )
    }

    /// Blend a source colour value over a backdrop's, both 0 (dark) to 1
    /// (light), as PDF's separable blend functions do.
    pub fn blend(self, backdrop: f32, source: f32) -> f32 {
        let (b, s) = (backdrop, source);
        let hard_light = |b: f32, s: f32| {
            if s <= 0.5 {
                b * 2.0 * s
            } else {
                let s = 2.0 * s - 1.0;
                b + s - b * s
            }
        };
        match self {
            Self::Normal | Self::Hue | Self::Saturation | Self::Color | Self::Luminosity => s,
            Self::Multiply => b * s,
            Self::Screen => b + s - b * s,
            Self::Overlay => hard_light(s, b),
            Self::HardLight => hard_light(b, s),
            Self::SoftLight => {
                if s <= 0.5 {
                    b - (1.0 - 2.0 * s) * b * (1.0 - b)
                } else {
                    let d = if b <= 0.25 {
                        ((16.0 * b - 12.0) * b + 4.0) * b
                    } else {
                        b.sqrt()
                    };
                    b + (2.0 * s - 1.0) * (d - b)
                }
            }
            Self::ColorDodge => {
                if b == 0.0 {
                    0.0
                } else if s >= 1.0 {
                    1.0
                } else {
                    (b / (1.0 - s)).min(1.0)
                }
            }
            Self::ColorBurn => {
                if b >= 1.0 {
                    1.0
                } else if s <= 0.0 {
                    0.0
                } else {
                    1.0 - ((1.0 - b) / s).min(1.0)
                }
            }
            Self::Darken => b.min(s),
            Self::Lighten => b.max(s),
            Self::Difference => (b - s).abs(),
            Self::Exclusion => b + s - 2.0 * b * s,
        }
        .clamp(0.0, 1.0)
    }

    /// Blend ink amounts, 0 (none) to 1 (solid), as PDF blends subtractive
    /// colorants: on their complements.
    pub fn blend_ink(self, backdrop: f32, source: f32) -> f32 {
        1.0 - self.blend(1.0 - backdrop, 1.0 - source)
    }
}

/// A drop shadow, as an item's DropShadowSetting describes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DropShadow {
    /// The shadow's colour; None is black, the specification's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<crate::Ink>,
    /// 0 to 1.
    pub opacity: f32,
    pub blend: BlendMode,
    /// Points right and down.
    pub x_offset: crate::Pt,
    pub y_offset: crate::Pt,
    /// The blur's size in points.
    pub size: crate::Pt,
    /// Spread and Noise as read, 0 to 1; Schist draws neither.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub spread: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub noise: f32,
    /// Whether the item hides the shadow beneath it, as it does by default.
    pub knocked_out: bool,
}

fn is_zero(value: &f32) -> bool {
    *value == 0.0
}

impl Default for DropShadow {
    /// The specification's defaults: black, 75 %, Multiply, 7 pt right and
    /// down, 5 pt soft, knocked out by the item.
    fn default() -> Self {
        Self {
            color: None,
            opacity: 0.75,
            blend: BlendMode::Multiply,
            x_offset: 7.0,
            y_offset: 7.0,
            size: 5.0,
            spread: 0.0,
            noise: 0.0,
            knocked_out: true,
        }
    }
}

impl DropShadow {
    /// The shadow's ink.
    pub fn ink(&self) -> crate::Ink {
        self.color.clone().unwrap_or_else(crate::Ink::black)
    }
}

/// Blur `data`, a `width` × `height` coverage, by a Gaussian of deviation
/// `sigma` pixels, approximated by three box blurs per axis.
pub fn blur(data: &mut [f32], width: usize, height: usize, sigma: f32) {
    if sigma <= 0.0 || width == 0 || height == 0 || data.len() != width * height {
        return;
    }
    // Three boxes of width w have a variance of 3·(w² − 1)/12.
    let box_width = ((4.0 * sigma * sigma + 1.0).sqrt().round() as usize).max(1);
    let radius = box_width / 2;
    if radius == 0 {
        return;
    }
    let mut line = Vec::new();
    for _ in 0..3 {
        for y in 0..height {
            line.clear();
            line.extend_from_slice(&data[y * width..(y + 1) * width]);
            box_line(&line, &mut data[y * width..(y + 1) * width], radius);
        }
        let mut column = vec![0.0; height];
        let mut out = vec![0.0; height];
        for x in 0..width {
            for y in 0..height {
                column[y] = data[y * width + x];
            }
            box_line(&column, &mut out, radius);
            for y in 0..height {
                data[y * width + x] = out[y];
            }
        }
    }
}

/// A running mean of `radius` either side, zero beyond the ends.
fn box_line(input: &[f32], output: &mut [f32], radius: usize) {
    let n = input.len();
    let span = (2 * radius + 1) as f32;
    let mut sum: f32 = input.iter().take(radius.min(n)).sum();
    for i in 0..n {
        if i + radius < n {
            sum += input[i + radius];
        }
        if i > radius {
            sum -= input[i - radius - 1];
        }
        output[i] = sum / span;
    }
}
