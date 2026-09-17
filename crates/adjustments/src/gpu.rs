//! Shared coefficient ABI for compositing and destructive adjustments.
use crate::Params;

pub const D_NONE: u32 = 0;
pub const D_HUE_SATURATION: u32 = 1;
pub const D_BLACK_WHITE: u32 = 2;
pub const D_THRESHOLD: u32 = 3;
pub const D_POSTERIZE: u32 = 4;
pub const D_COLOR_BALANCE: u32 = 5;
pub const D_VIBRANCE: u32 = 6;
pub const D_PHOTO_FILTER: u32 = 7;
pub const D_GRADIENT_MAP: u32 = 8;
pub const D_SELECTIVE_COLOR: u32 = 9;
pub const D_CHANNEL_MIXER: u32 = 10;
pub const D_WHITE_BALANCE: u32 = 11;

/// Variable records preserve arbitrary imported hue ranges and gradient stops.
/// Coefficients use the CPU's operand order, rather than resampling a 3D LUT.
pub fn direct_coeffs(params: &Params) -> Option<(u32, Vec<f32>)> {
    let flag = |v: bool| u8::from(v) as f32;
    Some(match params {
        Params::HueSaturation {
            hue,
            saturation,
            lightness,
            colorize,
            lightness_desaturates,
            reciprocal_saturation,
            ranges,
        } => {
            let mut out = vec![
                *hue,
                *saturation,
                *lightness,
                flag(*colorize),
                flag(*lightness_desaturates),
                flag(*reciprocal_saturation),
                ranges.len() as f32,
            ];
            for r in ranges {
                out.extend(r.bounds);
                out.extend([r.hue, r.saturation, r.lightness]);
            }
            (D_HUE_SATURATION, out)
        }
        Params::BlackWhite {
            reds,
            yellows,
            greens,
            cyans,
            blues,
            magentas,
        } => (
            D_BLACK_WHITE,
            [*reds, *yellows, *greens, *cyans, *blues, *magentas]
                .map(|v| v / 100.0)
                .to_vec(),
        ),
        Params::Threshold { level } => (D_THRESHOLD, vec![*level]),
        Params::Posterize { levels } => (D_POSTERIZE, vec![(*levels).clamp(2, 255) as f32]),
        Params::ColorBalance {
            shadows,
            midtones,
            highlights,
            preserve_luminosity,
        } => {
            let mut out = shadows.to_vec();
            out.extend(midtones);
            out.extend(highlights);
            out.push(flag(*preserve_luminosity));
            (D_COLOR_BALANCE, out)
        }
        Params::Vibrance {
            vibrance,
            saturation,
        } => {
            let t = (vibrance / 100.0).clamp(-1.0, 1.0);
            let mut out = vec![t, 1.0 + saturation / 100.0, t.max(0.0).powf(0.7)];
            out.extend(crate::color::VIBRANCE_BOOST);
            (D_VIBRANCE, out)
        }
        Params::PhotoFilter {
            color,
            density,
            preserve_luminosity,
        } => {
            let d = (density / 100.0).clamp(0.0, 1.0);
            (
                D_PHOTO_FILTER,
                vec![
                    color[0],
                    color[1],
                    color[2],
                    0.9 * d * d,
                    flag(*preserve_luminosity),
                ],
            )
        }
        Params::GradientMap {
            from,
            to,
            reverse,
            stops,
        } => {
            let mut out = vec![flag(*reverse), stops.len() as f32];
            out.extend(from);
            out.extend(to);
            for (position, color) in stops {
                out.push(*position);
                out.extend(color);
            }
            (D_GRADIENT_MAP, out)
        }
        Params::SelectiveColor { ranges, relative } => {
            let mut out = vec![flag(*relative)];
            out.extend(ranges.iter().flatten());
            (D_SELECTIVE_COLOR, out)
        }
        Params::ChannelMixer {
            red,
            green,
            blue,
            constant,
            monochrome,
        } => {
            let mut out = vec![flag(*monochrome)];
            for row in [red, green, blue, constant] {
                out.extend(row);
            }
            (D_CHANNEL_MIXER, out)
        }
        Params::WhiteBalance { warmth, tint } => {
            let (matrix, inverse, gains) =
                crate::white_balance::white_balance_coefficients(*warmth, *tint);
            let mut out: Vec<_> = matrix.into_iter().flatten().collect();
            out.extend(inverse.into_iter().flatten());
            out.extend(gains);
            (D_WHITE_BALANCE, out)
        }
        Params::Invert => (12, vec![]),
        Params::SolidColor { rgba } => (13, rgba[..3].to_vec()),
        Params::Exposure {
            exposure,
            offset,
            gamma,
        } => (
            14,
            vec![
                2f32.powf(*exposure),
                *offset,
                (1.0 / gamma.max(0.01)).clamp(0.01, 100.0),
            ],
        ),
        Params::BrightnessContrast {
            brightness,
            contrast,
        } => (
            15,
            vec![
                brightness / 100.0,
                if *contrast >= 0.0 {
                    1.0 / (1.0 - contrast / 100.0 * 0.99).max(1e-3)
                } else {
                    1.0 + contrast / 100.0
                },
            ],
        ),
        Params::Levels(l) => {
            let mut out = Vec::new();
            for c in [&l.rgb, &l.red, &l.green, &l.blue] {
                out.extend_from_slice(&[
                    c.input_black,
                    (c.input_white - c.input_black).max(1e-4),
                    if (c.gamma - 1.0).abs() < 1e-4 {
                        1.0
                    } else {
                        1.0 / c.gamma.max(1e-3)
                    },
                    c.output_black,
                    c.output_white - c.output_black,
                ]);
            }
            (16, out)
        }
        Params::Curves(c) => {
            let mut out = vec![0.0; 4];
            for (i, curve) in [&c.rgb, &c.red, &c.green, &c.blue].iter().enumerate() {
                out[i] = out.len() as f32;
                out.push(curve.points.len() as f32);
                for &(x, y) in &curve.points {
                    out.extend_from_slice(&[x, y]);
                }
            }
            (17, out)
        }
        Params::Unsupported => return None,
    })
}

pub static ADJUSTMENT: schist_fx::ComputeShader = schist_fx::ComputeShader {
    name: "destructive-adjustment",
    source: concat!(include_str!("gpu.wgsl"), include_str!("gpu_buffer.wgsl")),
};

pub fn buffer_program(params: &crate::Params, floats: usize) -> Option<schist_fx::ComputeProgram> {
    let (kind, mut args) = direct_coeffs(params)?;
    args.insert(0, kind as f32);
    let mut p = schist_fx::ComputeProgram::single(
        &ADJUSTMENT,
        args,
        floats,
        [(floats / 4) as u32, 1, 4],
        (floats / 4).saturating_mul(128),
    );
    p.steps[0].invocations = floats / 4;
    Some(p)
}
