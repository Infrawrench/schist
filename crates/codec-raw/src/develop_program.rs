//! Complete, resident RAW development for asynchronous callers and images that
//! fit one submission. Larger native jobs retain the existing banded kernels.
use super::*;
use schist_fx::{ComputeProgram, ComputeShader};
static GEOMETRY: ComputeShader =
    ComputeShader::new("raw-super-ccd", include_str!("super_ccd.wgsl"));

pub(super) fn program(
    raw: &RawImage,
    options: &DevelopOptions,
) -> Option<(ComputeProgram, usize, usize)> {
    if raw.data.len() > 8_388_608 || !matches!(raw.cpp, 1 | 3) {
        return None;
    }
    let (mut width, mut height) = (raw.width, raw.height);
    let wb = white_balance(raw, options);
    let levels = if raw.cpp == 3 {
        Levels::per_channel(raw, wb)
    } else {
        Levels::per_position(raw, wb)
    }
    .ok()?;
    let mut args = vec![
        0.0,
        raw.cpp as f32,
        levels.width as f32,
        levels.height as f32,
        0.0,
        levels.black.len() as f32,
    ];
    args.extend(&levels.black);
    args.extend(&levels.gain);
    let mut p = ComputeProgram::single(
        &DEVELOP_GPU,
        args,
        raw.data.len(),
        [width as u32, height as u32, raw.cpp as u32],
        raw.data.len().saturating_mul(100),
    );
    let mut crop = if options.crop {
        raw.crop
    } else {
        Rect {
            x: 0,
            y: 0,
            width,
            height,
        }
    };
    if raw.cpp == 1 {
        let mut cfa = raw.cfa.clone();
        if let Cfa::SuperCcd {
            row_staggered,
            fuji_width,
            ..
        } = raw.cfa
        {
            let bayer = raw.cfa.super_ccd_bayer((raw.crop.x, raw.crop.y))?;
            let line = fuji_width.checked_shl(u32::from(!row_staggered))?;
            if line != raw.crop.width {
                return None;
            }
            let (w, h) = super_ccd_sheared_size(row_staggered, fuji_width, raw.crop.height).ok()?;
            p.result = p.push(
                &GEOMETRY,
                p.result,
                p.result,
                vec![
                    0.0,
                    fuji_width as f32,
                    raw.crop.width as f32,
                    raw.crop.height as f32,
                    u8::from(row_staggered) as f32,
                    width as f32,
                    raw.crop.y as f32,
                    raw.crop.x as f32,
                ],
                w.checked_mul(h)?,
                [w as u32, h as u32, 1],
            );
            width = w;
            height = h;
            cfa = Cfa::Bayer(bayer);
        }
        let demosaic = crate::demosaic::gpu_program(width, height, &cfa, options.quality)?;
        p.result = p.append(&demosaic, p.result);
        if let Cfa::SuperCcd { fuji_width, .. } = raw.cfa {
            let (w, h) = super_ccd_output_size(fuji_width, height).ok()?;
            let count = w + h - 1;
            let mut args = vec![1.0, width as f32, height as f32, count as f32];
            let scale = 0.5f64.sqrt();
            args.extend((0..count).map(|i| {
                (fuji_width.saturating_sub(1) as f64 + (i as f64 - (w - 1) as f64) * scale) as f32
            }));
            args.extend((0..count).map(|i| (i as f64 * scale) as f32));
            p.result = p.push(
                &GEOMETRY,
                p.result,
                p.result,
                args,
                w.checked_mul(h)?.checked_mul(3)?,
                [w as u32, h as u32, 3],
            );
            width = w;
            height = h;
            crop = Rect {
                x: 0,
                y: 0,
                width,
                height,
            };
        }
    }
    let orientation = if options.orient {
        raw.orientation
    } else {
        Orientation::Normal
    };
    let (w, h) = if orientation.transposes() {
        (crop.height, crop.width)
    } else {
        (crop.width, crop.height)
    };
    let mut args = vec![
        1.0,
        width as f32,
        crop.width as f32,
        crop.height as f32,
        crop.x as f32,
        crop.y as f32,
        orientation as u32 as f32,
    ];
    args.extend(
        srgb_matrix(raw, options)
            .unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
            .into_iter()
            .flatten(),
    );
    p.result = p.push(
        &DEVELOP_GPU,
        p.result,
        p.result,
        args,
        w.checked_mul(h)?.checked_mul(3)?,
        [w as u32, h as u32, 3],
    );
    Some((p, w, h))
}

fn input(raw: &RawImage) -> std::borrow::Cow<'_, [f32]> {
    match &raw.data {
        RawData::U16(v) => v.iter().map(|&v| v as f32).collect::<Vec<_>>().into(),
        RawData::F32(v) => v.as_slice().into(),
    }
}

pub(super) fn run(raw: &RawImage, options: &DevelopOptions) -> Option<Developed> {
    if !schist_fx::backend().compute_available(raw.data.len().saturating_mul(100)) {
        return None;
    }
    let (program, width, height) = program(raw, options)?;
    let input = input(raw);
    let rgb = schist_fx::try_compute(&input, &program)?;
    Some(Developed { width, height, rgb })
}

/// Await complete RAW processing without blocking a browser event loop on GPU
/// completion. Invalid captures return their normal error; unavailable or
/// oversized graphs retain the established developer as fallback.
pub async fn develop_async(
    raw: &RawImage,
    options: &DevelopOptions,
    backend: &impl schist_fx::AsyncCompute,
) -> Result<Developed> {
    raw.validate()?;
    if let Some((program, width, height)) = program(raw, options) {
        let input = input(raw);
        if let Some(rgb) = backend
            .compute_async(schist_fx::ComputeJob {
                input: &input,
                program: &program,
            })
            .await
        {
            return Ok(Developed { width, height, rgb });
        }
    }
    develop(raw, options)
}
