//! Android's flexible YUV planes can be planar or interleaved, padded and cropped.
//! Keep conversion in Rust so the addressing and colour ranges have host tests.
use super::*;

pub(super) fn convert(
    time: f64,
    layout: &[i32; 12],
    planes: [&[u8]; 3],
    edge: u32,
) -> Result<Frame> {
    let [width, height, left, top, yr, yp, ur, up, vr, vp, standard, range] = *layout;
    let width = u32::try_from(width)?;
    let height = u32::try_from(height)?;
    pixel_count(width, height)?;
    ensure!(left >= 0 && top >= 0, "{}", t("video.decode_failed"));
    let layouts = [(yr, yp, 1), (ur, up, 2), (vr, vp, 2)];
    for ((row, pixel, subsample), bytes) in layouts.iter().zip(planes) {
        ensure!(*row > 0 && *pixel > 0, "{}", t("video.decode_failed"));
        let last_x = (left as u64 + width as u64 - 1) / *subsample as u64;
        let last_y = (top as u64 + height as u64 - 1) / *subsample as u64;
        let end = last_y * *row as u64 + last_x * *pixel as u64;
        ensure!(end < bytes.len() as u64, "{}", t("video.decode_failed"));
    }
    // Sample directly at preview resolution: full-size capture uses every
    // pixel, while a phone playing 4K need not allocate a 4K RGBA image.
    let scale = if edge > 0 {
        (edge as f64 / width.max(height) as f64).min(1.0)
    } else {
        1.0
    };
    let out_w = (width as f64 * scale).round().max(1.0) as u32;
    let out_h = (height as f64 * scale).round().max(1.0) as u32;
    let mut rgba = Vec::with_capacity(pixel_count(out_w, out_h)? * 4);
    let (kr, kb) = match standard {
        1 => (0.2126, 0.0722),
        6 => (0.2627, 0.0593),
        _ => (0.299, 0.114),
    };
    let kg = 1.0 - kr - kb;
    for y in 0..out_h {
        let sy = top as usize + (y as u64 * height as u64 / out_h as u64) as usize;
        for x in 0..out_w {
            let sx = left as usize + (x as u64 * width as u64 / out_w as u64) as usize;
            let yy = planes[0][sy * yr as usize + sx * yp as usize] as f64;
            let u = planes[1][(sy / 2) * ur as usize + (sx / 2) * up as usize] as f64 - 128.0;
            let v = planes[2][(sy / 2) * vr as usize + (sx / 2) * vp as usize] as f64 - 128.0;
            let (luma, u, v) = if range == 1 {
                (yy, u, v)
            } else {
                (
                    (yy - 16.0) * 255.0 / 219.0,
                    u * 255.0 / 224.0,
                    v * 255.0 / 224.0,
                )
            };
            let r = luma + 2.0 * (1.0 - kr) * v;
            let b = luma + 2.0 * (1.0 - kb) * u;
            let g = luma - 2.0 * kb * (1.0 - kb) / kg * u - 2.0 * kr * (1.0 - kr) / kg * v;
            rgba.extend_from_slice(&[
                r.round().clamp(0.0, 255.0) as u8,
                g.round().clamp(0.0, 255.0) as u8,
                b.round().clamp(0.0, 255.0) as u8,
                255,
            ]);
        }
    }
    Ok(Frame {
        time,
        width: out_w,
        height: out_h,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn padded_cropped_interleaved_planes_and_ranges() {
        let layout = [2, 2, 1, 1, 4, 1, 4, 2, 4, 2, 2, 2];
        let y = [16, 16, 16, 0, 16, 235, 16, 0, 16, 16, 235];
        let chroma = [128, 99, 128, 99, 128, 99, 128];
        let frame = convert(0.25, &layout, [&y, &chroma, &chroma], 0).unwrap();
        assert_eq!(
            frame.rgba,
            [255, 255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255]
        );
        assert!(convert(0.0, &layout, [&y[..10], &chroma, &chroma], 0).is_err());
        let full = [1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1];
        assert_eq!(
            convert(0.0, &full, [&[128], &[128], &[128]], 0)
                .unwrap()
                .rgba,
            [128, 128, 128, 255]
        );
    }
    #[test]
    fn limited_bt601_red_and_preview_dimensions() {
        let layout = [2, 2, 0, 0, 2, 1, 1, 1, 1, 1, 2, 2];
        let frame = convert(0.0, &layout, [&[81; 4], &[90], &[240]], 1).unwrap();
        assert_eq!((frame.width, frame.height), (1, 1));
        assert!(frame.rgba[0] > 250 && frame.rgba[1] < 3 && frame.rgba[2] < 3);
    }
}
