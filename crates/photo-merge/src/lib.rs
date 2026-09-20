//! Bounded, cancellable translation registration and photographic merging.
//! Inputs and display outputs are straight-alpha sRGB; HDR is reconstructed
//! in linear light, optionally mapped with a luminance-based Reinhard curve.
//! No rotation, perspective, lens-distortion or moving-subject correction.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub const MAX_PIXELS: usize = 64_000_000;
pub const MAX_TOTAL_PIXELS: usize = 128_000_000;
pub const MAX_IMAGES: usize = 16;

#[derive(Debug, Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<f32>,
}

impl Image {
    pub fn validate(&self) -> Result<(), Error> {
        let len = pixels(self.width, self.height)?;
        if self.rgba.len() != len * 4
            || self
                .rgba
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    fn at(&self, x: i32, y: i32) -> Option<&[f32]> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        Some(&self.rgba[i..i + 4])
    }
}

pub fn pixels(width: u32, height: u32) -> Result<usize, Error> {
    let count = (width as usize)
        .checked_mul(height as usize)
        .ok_or(Error::TooLarge)?;
    if width == 0 || height == 0 || width > 30_000 || height > 30_000 || count > MAX_PIXELS {
        return Err(Error::TooLarge);
    }
    Ok(count)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Align,
    Focus,
    Hdr,
    Panorama,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub mode: Mode,
    pub align: bool,
    /// Intersection crop for stacks; ignored for panoramas.
    pub crop: bool,
    /// Radius of the box-filtered absolute Laplacian focus measure, 1..=16.
    pub focus_radius: u32,
    /// Exposure offsets in stops. +1 means twice as much light reached sensor.
    pub exposure_ev: Vec<f32>,
    /// Post-reconstruction display exposure in stops, -12..=12.
    pub tone_ev: f32,
    /// Compress HDR for display; false retains extended-range sRGB-encoded
    /// scene radiance for a 32-bit document instead of discarding highlights.
    pub tone_map: bool,
    pub feather: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Align,
            align: true,
            crop: false,
            focus_radius: 3,
            exposure_ev: Vec::new(),
            tone_ev: 0.0,
            tone_map: true,
            feather: 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
    TooLarge,
    NoMatch,
    Cancelled,
}

/// Shared with UI. Progress is monotonic, 0..=1000; cancellation is checked
/// within registration candidates and every output row.
#[derive(Debug, Default)]
pub struct Control {
    pub cancelled: AtomicBool,
    pub progress: AtomicUsize,
}

impl Control {
    pub fn check(&self) -> Result<(), Error> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
    fn report(&self, n: usize) {
        self.progress.fetch_max(n.min(1000), Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Offset {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug)]
pub struct Output {
    pub width: u32,
    pub height: u32,
    /// Source top-left positions in the output canvas.
    pub offsets: Vec<Offset>,
    /// Alignment returns positions only so callers can retain separate layers.
    pub merged: Option<Image>,
}

fn alloc<T: Clone>(len: usize, value: T) -> Result<Vec<T>, Error> {
    let mut v = Vec::new();
    v.try_reserve_exact(len).map_err(|_| Error::TooLarge)?;
    v.resize(len, value);
    Ok(v)
}

fn luma(p: &[f32]) -> f32 {
    0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
}
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

struct Gray {
    width: i32,
    height: i32,
    values: Vec<f32>,
}

impl Gray {
    fn from_image(image: &Image, step: u32, control: &Control) -> Result<Self, Error> {
        let width = image.width.div_ceil(step) as i32;
        let height = image.height.div_ceil(step) as i32;
        let mut values = alloc(width as usize * height as usize, f32::NAN)?;
        for y in 0..height {
            control.check()?;
            for x in 0..width {
                // Area averaging suppresses aliasing and makes defocused shots
                // register against the same large-scale scene structures.
                let mut total = 0.0;
                let mut weight = 0.0;
                for yy in y as u32 * step..((y as u32 + 1) * step).min(image.height) {
                    for xx in x as u32 * step..((x as u32 + 1) * step).min(image.width) {
                        let p = image.at(xx as i32, yy as i32).unwrap();
                        total += luma(p) * p[3];
                        weight += p[3];
                    }
                }
                if weight > 0.1 {
                    values[(y * width + x) as usize] = total / weight;
                }
            }
        }
        Ok(Self {
            width,
            height,
            values,
        })
    }
    fn at(&self, x: i32, y: i32) -> f32 {
        self.values[(y * self.width + x) as usize]
    }
}

/// Zero-mean normalized correlation is invariant to affine brightness changes.
/// Require substantial geometric overlap and texture, rejecting blank/unrelated
/// shots instead of silently manufacturing a plausible translation.
fn correlation(a: &Gray, b: &Gray, dx: i32, dy: i32, overlap: f64) -> Option<f64> {
    let x0 = 0.max(dx);
    let y0 = 0.max(dy);
    let x1 = a.width.min(dx + b.width);
    let y1 = a.height.min(dy + b.height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let area = (x1 - x0) as f64 * (y1 - y0) as f64;
    let smaller = (a.width as f64 * a.height as f64).min(b.width as f64 * b.height as f64);
    if area < smaller * overlap {
        return None;
    }
    let stride = ((area / 768.0).sqrt() as usize).max(1);
    let (mut n, mut sa, mut sb, mut saa, mut sbb, mut sab) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for y in (y0..y1).step_by(stride) {
        for x in (x0..x1).step_by(stride) {
            let av = a.at(x, y) as f64;
            let bv = b.at(x - dx, y - dy) as f64;
            if !av.is_finite() || !bv.is_finite() {
                continue;
            }
            n += 1.0;
            sa += av;
            sb += bv;
            saa += av * av;
            sbb += bv * bv;
            sab += av * bv;
        }
    }
    if n < 24.0 {
        return None;
    }
    let va = saa - sa * sa / n;
    let vb = sbb - sb * sb / n;
    if va < n * 1e-6 || vb < n * 1e-6 {
        return None;
    }
    Some((sab - sa * sb / n) / (va * vb).sqrt())
}

/// Coarse exhaustive search plus a beam of eight translations refined at each
/// scale. Integer-pixel translation is the sole supported registration model.
pub fn register(
    a: &Image,
    b: &Image,
    panorama: bool,
    control: &Control,
) -> Result<(Offset, f64), Error> {
    a.validate()?;
    b.validate()?;
    let max_dim = a.width.max(a.height).max(b.width).max(b.height);
    let mut step = 1;
    while max_dim.div_ceil(step) > 64 {
        step *= 2;
    }
    let overlap = if panorama { 0.25 } else { 0.60 };
    let mut beam: Vec<(Offset, f64)> = Vec::new();
    loop {
        control.check()?;
        let ga = Gray::from_image(a, step, control)?;
        let gb = Gray::from_image(b, step, control)?;
        let mut candidates = Vec::new();
        if beam.is_empty() {
            for y in -gb.height + 1..ga.height {
                for x in -gb.width + 1..ga.width {
                    candidates.push(Offset { x, y });
                }
            }
        } else {
            for (offset, _) in &beam {
                for y in offset.y * 2 - 2..=offset.y * 2 + 2 {
                    for x in offset.x * 2 - 2..=offset.x * 2 + 2 {
                        candidates.push(Offset { x, y });
                    }
                }
            }
            candidates.sort_by_key(|o| (o.x, o.y));
            candidates.dedup();
        }
        let mut next = Vec::new();
        for offset in candidates {
            control.check()?;
            if let Some(score) = correlation(&ga, &gb, offset.x, offset.y, overlap) {
                next.push((offset, score));
            }
        }
        next.sort_by(|a, b| b.1.total_cmp(&a.1));
        next.truncate(8);
        if next.is_empty() {
            return Err(Error::NoMatch);
        }
        beam = next;
        if step == 1 {
            break;
        }
        step /= 2;
    }
    let best = beam[0];
    if best.1 < 0.65 {
        Err(Error::NoMatch)
    } else {
        Ok(best)
    }
}

fn bounds(images: &[Image], offsets: &[Offset], crop: bool) -> Result<(i32, i32, u32, u32), Error> {
    let mut left = 0;
    let mut top = 0;
    let mut right = images[0].width as i32;
    let mut bottom = images[0].height as i32;
    for (image, offset) in images.iter().zip(offsets).skip(1) {
        if crop {
            left = left.max(offset.x);
            top = top.max(offset.y);
            right = right.min(offset.x + image.width as i32);
            bottom = bottom.min(offset.y + image.height as i32);
        } else {
            left = left.min(offset.x);
            top = top.min(offset.y);
            right = right.max(offset.x + image.width as i32);
            bottom = bottom.max(offset.y + image.height as i32);
        }
    }
    if right <= left || bottom <= top {
        return Err(Error::NoMatch);
    }
    let width = (right - left) as u32;
    let height = (bottom - top) as u32;
    pixels(width, height)?;
    Ok((left, top, width, height))
}

/// Validate before any working allocation. The input pixel budget also bounds
/// focus maps and registration pyramids; output dimensions are checked again
/// after registration, before allocating the panorama canvas.
pub fn merge(images: &[Image], options: &Options, control: &Control) -> Result<Output, Error> {
    control.check()?;
    if !(2..=MAX_IMAGES).contains(&images.len())
        || !(1..=16).contains(&options.focus_radius)
        || !options.tone_ev.is_finite()
        || options.tone_ev.abs() > 12.0
        || options.feather > 4096
    {
        return Err(Error::Invalid);
    }
    let mut total = 0usize;
    for image in images {
        image.validate()?;
        total = total
            .checked_add(pixels(image.width, image.height)?)
            .ok_or(Error::TooLarge)?;
    }
    if total > MAX_TOTAL_PIXELS {
        return Err(Error::TooLarge);
    }
    if options.mode == Mode::Hdr
        && (options.exposure_ev.len() != images.len()
            || options
                .exposure_ev
                .iter()
                .any(|ev| !ev.is_finite() || ev.abs() > 20.0))
    {
        return Err(Error::Invalid);
    }
    let mut offsets = vec![Offset::default()];
    for i in 1..images.len() {
        control.check()?;
        let offset = if options.align || matches!(options.mode, Mode::Align | Mode::Panorama) {
            let reference = if options.mode == Mode::Panorama {
                i - 1
            } else {
                0
            };
            let (local, _) = register(
                &images[reference],
                &images[i],
                options.mode == Mode::Panorama,
                control,
            )?;
            Offset {
                x: offsets[reference].x + local.x,
                y: offsets[reference].y + local.y,
            }
        } else {
            Offset::default()
        };
        offsets.push(offset);
        control.report(400 * i / images.len());
    }
    let (left, top, width, height) = bounds(
        images,
        &offsets,
        options.crop && options.mode != Mode::Panorama,
    )?;
    for offset in &mut offsets {
        offset.x -= left;
        offset.y -= top;
    }
    if options.mode == Mode::Align {
        control.report(1000);
        return Ok(Output {
            width,
            height,
            offsets,
            merged: None,
        });
    }
    let mut rgba = alloc(pixels(width, height)? * 4, 0.0f32)?;
    let mut focus = Vec::new();
    if options.mode == Mode::Focus {
        for (index, image) in images.iter().enumerate() {
            focus.push(focus_map(image, options.focus_radius, control)?);
            control.report(400 + 100 * (index + 1) / images.len());
        }
    }
    for y in 0..height as i32 {
        control.check()?;
        for x in 0..width as i32 {
            let dst = &mut rgba[(y as usize * width as usize + x as usize) * 4..][..4];
            let mut sum = [0.0f32; 3];
            let mut weights = [0.0f32; 3];
            let mut alpha = 0.0f32;
            let mut best_focus = -1.0;
            let mut fallback = [0.0f32; 3];
            let mut fallback_weight = [0.0f32; 3];
            for (i, (image, offset)) in images.iter().zip(&offsets).enumerate() {
                let sx = x - offset.x;
                let sy = y - offset.y;
                let Some(p) = image.at(sx, sy) else {
                    continue;
                };
                if p[3] <= 0.0 {
                    continue;
                }
                alpha = alpha.max(p[3]);
                match options.mode {
                    Mode::Focus => {
                        let score =
                            focus[i][sy as usize * image.width as usize + sx as usize] * p[3];
                        if score > best_focus || (score == best_focus && p[3] > dst[3]) {
                            dst.copy_from_slice(p);
                            best_focus = score;
                        }
                    }
                    Mode::Panorama => {
                        let edge = (sx + 1)
                            .min(sy + 1)
                            .min(image.width as i32 - sx)
                            .min(image.height as i32 - sy);
                        let w = p[3] * (edge as f32 / options.feather.max(1) as f32).min(1.0);
                        for c in 0..3 {
                            sum[c] += srgb_to_linear(p[c]) * w;
                            weights[c] += w;
                        }
                    }
                    Mode::Hdr => {
                        let exposure = options.exposure_ev[i].exp2();
                        for c in 0..3 {
                            let radiance = srgb_to_linear(p[c]) / exposure;
                            let w = (1.0 - (2.0 * p[c] - 1.0).abs()).max(0.0) * p[3];
                            sum[c] += radiance * w;
                            weights[c] += w;
                            // Every exposure clipped: select the least clipped
                            // measurement (shortest for white, longest for black).
                            let quality = if p[c] >= 0.5 {
                                1.0 / exposure
                            } else {
                                exposure
                            };
                            if quality > fallback_weight[c] {
                                fallback[c] = radiance;
                                fallback_weight[c] = quality;
                            }
                        }
                    }
                    Mode::Align => unreachable!(),
                }
            }
            if options.mode == Mode::Focus || alpha == 0.0 {
                continue;
            }
            let mut linear = [0.0; 3];
            for c in 0..3 {
                linear[c] = if weights[c] > 1e-8 {
                    sum[c] / weights[c]
                } else {
                    fallback[c]
                };
            }
            if options.mode == Mode::Hdr && options.tone_map {
                linear = tone_map(linear, options.tone_ev);
            }
            for c in 0..3 {
                let v = linear_to_srgb(linear[c].max(0.0));
                dst[c] = if options.mode == Mode::Hdr && !options.tone_map {
                    v
                } else {
                    v.min(1.0)
                };
            }
            dst[3] = alpha;
        }
        control.report(500 + 500 * (y as usize + 1) / height as usize);
    }
    Ok(Output {
        width,
        height,
        offsets,
        merged: Some(Image {
            width,
            height,
            rgba,
        }),
    })
}

/// Global luminance compression; RGB ratios are preserved before display gamut
/// clipping. Returns linear samples, before sRGB encoding.
pub fn tone_map(mut radiance: [f32; 3], ev: f32) -> [f32; 3] {
    let exposure = ev.exp2();
    let luminance = luma(&radiance) * exposure;
    let factor = exposure / (1.0 + luminance);
    for v in &mut radiance {
        *v *= factor;
    }
    radiance
}

fn focus_map(image: &Image, radius: u32, control: &Control) -> Result<Vec<f32>, Error> {
    let w = image.width as usize;
    let h = image.height as usize;
    let mut map = alloc(w * h, 0.0f32)?;
    // Separable box filtering keeps memory linear and avoids integral-image
    // cancellation errors on high resolution images with weak texture.
    for y in 0..h {
        control.check()?;
        for x in 0..w {
            let p = image.at(x as i32, y as i32).unwrap();
            if p[3] < 0.1 {
                continue;
            }
            let center = luma(p);
            let neighbor = |xx: i32, yy: i32| {
                image
                    .at(xx, yy)
                    .filter(|p| p[3] >= 0.1)
                    .map(luma)
                    .unwrap_or(center)
            };
            map[y * w + x] = (4.0 * center
                - neighbor(x as i32 - 1, y as i32)
                - neighbor(x as i32 + 1, y as i32)
                - neighbor(x as i32, y as i32 - 1)
                - neighbor(x as i32, y as i32 + 1))
            .abs();
        }
    }
    let mut filtered = alloc(w * h, 0.0f32)?;
    let r = radius as usize;
    for y in 0..h {
        control.check()?;
        let mut sum: f32 = map[y * w..y * w + (r + 1).min(w)].iter().sum();
        for x in 0..w {
            if x > 0 {
                if x + r < w {
                    sum += map[y * w + x + r];
                }
                if x > r {
                    sum -= map[y * w + x - r - 1];
                }
            }
            filtered[y * w + x] = sum / ((x + r + 1).min(w) - x.saturating_sub(r)) as f32;
        }
    }
    for x in 0..w {
        control.check()?;
        let mut sum = (0..(r + 1).min(h))
            .map(|y| filtered[y * w + x])
            .sum::<f32>();
        for y in 0..h {
            if y > 0 {
                if y + r < h {
                    sum += filtered[(y + r) * w + x];
                }
                if y > r {
                    sum -= filtered[(y - r - 1) * w + x];
                }
            }
            map[y * w + x] = (sum / ((y + r + 1).min(h) - y.saturating_sub(r)) as f32).max(0.0);
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests;
