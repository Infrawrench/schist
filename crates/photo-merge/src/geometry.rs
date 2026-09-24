//! Bounded direct projective registration. Models map reference pixels to source
//! pixels. Optimisation assumes modest viewpoint changes and a textured plane.
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct Transform(pub [f64; 9]);
impl Transform {
    pub const IDENTITY: Self = Self([1., 0., 0., 0., 1., 0., 0., 0., 1.]);
    pub fn point(self, x: f64, y: f64) -> Option<(f64, f64)> {
        let h = self.0;
        let z = h[6] * x + h[7] * y + h[8];
        if !z.is_finite() || z <= 0.05 {
            return None;
        }
        let p = (
            (h[0] * x + h[1] * y + h[2]) / z,
            (h[3] * x + h[4] * y + h[5]) / z,
        );
        (p.0.is_finite() && p.1.is_finite()).then_some(p)
    }
    fn inverse(self) -> Result<Self, Error> {
        let m = self.0;
        let a = [
            m[4] * m[8] - m[5] * m[7],
            m[2] * m[7] - m[1] * m[8],
            m[1] * m[5] - m[2] * m[4],
            m[5] * m[6] - m[3] * m[8],
            m[0] * m[8] - m[2] * m[6],
            m[2] * m[3] - m[0] * m[5],
            m[3] * m[7] - m[4] * m[6],
            m[1] * m[6] - m[0] * m[7],
            m[0] * m[4] - m[1] * m[3],
        ];
        let determinant = m[0] * a[0] + m[1] * a[3] + m[2] * a[6];
        if !determinant.is_finite()
            || determinant.abs() < 1e-10
            || a.iter().any(|v| !v.is_finite())
            || a[8].abs() < 1e-8
        {
            return Err(Error::NoMatch);
        }
        Ok(Self(a.map(|v| v / a[8])))
    }
}

/// Premultiplied-alpha bilinear reconstruction avoids color fringes at holes.
pub fn sample(image: &Image, x: f64, y: f64) -> [f32; 4] {
    let mut out = [0.; 4];
    if !x.is_finite()
        || !y.is_finite()
        || x < -1.0
        || y < -1.0
        || x >= image.width as f64
        || y >= image.height as f64
    {
        return out;
    }
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = (x - x.floor()) as f32;
    let fy = (y - y.floor()) as f32;
    for (dx, wx) in [(0, 1. - fx), (1, fx)] {
        for (dy, wy) in [(0, 1. - fy), (1, fy)] {
            if let Some(p) = image.at(ix + dx, iy + dy) {
                let w = wx * wy * p[3];
                for c in 0..3 {
                    out[c] += p[c] * w;
                }
                out[3] += w;
            }
        }
    }
    if out[3] > 0. {
        let alpha = out[3];
        for channel in &mut out[..3] {
            *channel /= alpha;
        }
    }
    out.map(|v| v.clamp(0.0, 1.0))
}

pub fn cylindrical(image: &Image, focal_ratio: f32, control: &Control) -> Result<Image, Error> {
    image.validate()?;
    if !focal_ratio.is_finite() || !(0.3..=5.).contains(&focal_ratio) {
        return Err(Error::Invalid);
    }
    let f = image.width as f64 * focal_ratio as f64;
    let cx = (image.width - 1) as f64 / 2.;
    let cy = (image.height - 1) as f64 / 2.;
    let mut rgba = alloc(image.rgba.len(), 0.)?;
    for y in 0..image.height {
        control.check()?;
        for x in 0..image.width {
            let theta = (x as f64 - cx) / f;
            if theta.abs() >= std::f64::consts::FRAC_PI_2 {
                continue;
            }
            let p = sample(
                image,
                f * theta.tan() + cx,
                (y as f64 - cy) / theta.cos() + cy,
            );
            let i = (y as usize * image.width as usize + x as usize) * 4;
            rgba[i..i + 4].copy_from_slice(&p);
        }
    }
    Ok(Image {
        width: image.width,
        height: image.height,
        rgba,
    })
}

fn score(a: &Image, b: &Image, h: Transform, step: usize) -> Option<f64> {
    let (mut n, mut sa, mut sb, mut aa, mut bb, mut ab) = (0., 0., 0., 0., 0., 0.);
    let mut count = 0;
    for y in (0..a.height).step_by(step) {
        for x in (0..a.width).step_by(step) {
            let p = a.at(x as i32, y as i32).unwrap();
            if p[3] < 0.1 {
                continue;
            }
            count += 1;
            let Some((xx, yy)) = h.point(x as f64, y as f64) else {
                continue;
            };
            let q = sample(b, xx, yy);
            if q[3] < 0.1 {
                continue;
            }
            let av = luma(p) as f64;
            let bv = luma(&q) as f64;
            n += 1.;
            sa += av;
            sb += bv;
            aa += av * av;
            bb += bv * bv;
            ab += av * bv;
        }
    }
    let va = aa - sa * sa / n;
    let vb = bb - sb * sb / n;
    if n < 24. || n < (count as f64) * 0.6 || va < n * 1e-6 || vb < n * 1e-6 {
        None
    } else {
        Some((ab - sa * sb / n) / (va * vb).sqrt())
    }
}

/// Direct normalised-correlation homography fit. Rotation seeds cover ±15°;
/// coordinate descent estimates scale, shear, translation and perspective.
pub fn register_projective(a: &Image, b: &Image, control: &Control) -> Result<Transform, Error> {
    a.validate()?;
    b.validate()?;
    let dim = a.width.max(a.height).max(b.width).max(b.height) as f64;
    let step = (dim / 96.).ceil().max(1.) as usize;
    let mut best = Transform::IDENTITY;
    let mut quality = -1.;
    let offset = register(a, b, false, control)
        .map(|v| v.0)
        .unwrap_or_default();
    for degrees in (-15..=15).step_by(3) {
        control.check()?;
        let angle = (degrees as f64).to_radians();
        let (s, c) = angle.sin_cos();
        let cx = (a.width - 1) as f64 / 2.;
        let cy = (a.height - 1) as f64 / 2.;
        let h = Transform([
            c,
            -s,
            cx - c * cx + s * cy - offset.x as f64,
            s,
            c,
            cy - s * cx - c * cy - offset.y as f64,
            0.,
            0.,
            1.,
        ]);
        if let Some(q) = score(a, b, h, step) {
            if q > quality {
                quality = q;
                best = h;
            }
        }
    }
    let mut scale = 4.0 * (dim / 160.0).max(1.0);
    while scale >= 0.125 {
        let delta = [
            scale / dim,
            scale / dim,
            scale,
            scale / dim,
            scale / dim,
            scale,
            scale / (dim * dim),
            scale / (dim * dim),
        ];
        for _ in 0..48 {
            let mut changed = false;
            for (k, amount) in delta.iter().enumerate() {
                for sign in [-1., 1.] {
                    control.check()?;
                    let mut h = best;
                    h.0[k] += sign * amount;
                    if let Some(q) = score(a, b, h, step) {
                        if q > quality + 1e-8 {
                            quality = q;
                            best = h;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        scale *= 0.5;
    }
    if quality < 0.75 || score(a, b, best, step + 1).is_none_or(|q| q < 0.75) {
        Err(Error::NoMatch)
    } else {
        Ok(best)
    }
}

pub(super) fn prepare(
    images: &[Image],
    options: &Options,
    control: &Control,
) -> Result<Vec<Image>, Error> {
    if options.cylindrical {
        return images
            .iter()
            .map(|im| cylindrical(im, options.focal_ratio, control))
            .collect();
    }
    // Stack registration to first image. Panorama chaining remains translation
    // after cylindrical projection; projective panorama is intentionally disabled.
    let mut transforms = vec![Transform::IDENTITY];
    for (index, image) in images.iter().enumerate().skip(1) {
        transforms.push(register_projective(&images[0], image, control)?);
        control.report(300 * index / images.len());
    }
    let (mut left, mut top, mut right, mut bottom) =
        (0f64, 0f64, images[0].width as f64, images[0].height as f64);
    for (im, h) in images.iter().zip(&transforms) {
        let inverse = h.inverse()?;
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for (x, y) in [
            (0., 0.),
            (im.width as f64, 0.),
            (0., im.height as f64),
            (im.width as f64, im.height as f64),
        ] {
            let (x, y) = inverse.point(x, y).ok_or(Error::NoMatch)?;
            xs.push(x);
            ys.push(y);
        }
        let x0 = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let x1 = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let y0 = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let y1 = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if options.crop {
            left = left.max(x0);
            top = top.max(y0);
            right = right.min(x1);
            bottom = bottom.min(y1);
        } else {
            left = left.min(x0);
            top = top.min(y0);
            right = right.max(x1);
            bottom = bottom.max(y1);
        }
    }
    left = left.floor();
    top = top.floor();
    right = right.ceil();
    bottom = bottom.ceil();
    if right <= left || bottom <= top || right - left > 30000. || bottom - top > 30000. {
        return Err(Error::TooLarge);
    }
    let mut width = (right - left) as u32;
    let mut height = (bottom - top) as u32;
    pixels(width, height)?;
    if options.crop {
        let rect = common_rectangle(images, &transforms, left, top, width, height, control)?;
        left += rect.0 as f64;
        top += rect.1 as f64;
        width = rect.2;
        height = rect.3;
    }
    let count = pixels(width, height)?;
    if count.checked_mul(images.len()).ok_or(Error::TooLarge)? > MAX_TOTAL_PIXELS {
        return Err(Error::TooLarge);
    }
    images
        .iter()
        .zip(transforms)
        .map(|(im, h)| {
            let mut rgba = alloc(count * 4, 0.)?;
            for y in 0..height {
                control.check()?;
                for x in 0..width {
                    if let Some((xx, yy)) = h.point(x as f64 + left, y as f64 + top) {
                        let i = (y as usize * width as usize + x as usize) * 4;
                        rgba[i..i + 4].copy_from_slice(&sample(im, xx, yy));
                    }
                }
            }
            Ok(Image {
                width,
                height,
                rgba,
            })
        })
        .collect()
}

/// Largest integer rectangle contained in every transformed source footprint.
/// Histogram scan is linear in canvas area; alpha holes are preserved rather
/// than changing geometry according to a subject's transparency.
fn common_rectangle(
    images: &[Image],
    transforms: &[Transform],
    left: f64,
    top: f64,
    width: u32,
    height: u32,
    control: &Control,
) -> Result<(u32, u32, u32, u32), Error> {
    let mut heights = alloc(width as usize, 0u32)?;
    let mut stack: Vec<(usize, u32)> = Vec::new();
    stack
        .try_reserve_exact(width as usize)
        .map_err(|_| Error::TooLarge)?;
    let mut best = (0, 0, 0, 0);
    let mut area = 0u64;
    for y in 0..height {
        control.check()?;
        for x in 0..width {
            let covered = images.iter().zip(transforms).all(|(im, h)| {
                h.point(x as f64 + left, y as f64 + top)
                    .is_some_and(|(xx, yy)| {
                        xx >= 0.
                            && yy >= 0.
                            && xx <= (im.width - 1) as f64
                            && yy <= (im.height - 1) as f64
                    })
            });
            heights[x as usize] = if covered { heights[x as usize] + 1 } else { 0 };
        }
        stack.clear();
        for x in 0..=width as usize {
            let h = heights.get(x).copied().unwrap_or(0);
            let mut start = x;
            while let Some(&(xx, hh)) = stack.last() {
                if hh <= h {
                    break;
                }
                stack.pop();
                start = xx;
                let candidate = (x - xx) as u64 * hh as u64;
                if candidate > area {
                    area = candidate;
                    best = (xx as u32, y + 1 - hh, (x - xx) as u32, hh);
                }
            }
            if h > 0 && stack.last().is_none_or(|&(_, hh)| hh < h) {
                stack.push((start, h));
            }
        }
    }
    if area == 0 {
        Err(Error::NoMatch)
    } else {
        Ok(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singular_inverse_and_nonfinite_sampling_are_safe() {
        // Bottom-right cofactor is nonzero, but determinant is zero.
        assert_eq!(
            Transform([1., 0., 0., 0., 1., 0., 0., 0., 0.])
                .inverse()
                .unwrap_err(),
            Error::NoMatch
        );
        let image = Image {
            width: 1,
            height: 1,
            rgba: vec![1., 1., 1., 1.],
        };
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e100, -1e100] {
            assert_eq!(sample(&image, x, 0.), [0.; 4]);
        }
    }
}
