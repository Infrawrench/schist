//! How `SPOT_HEAL_MAX_THICKNESS` was chosen.
//!
//! Punches brush-stroke holes of increasing width into photographs, fills
//! each with both of the Remove tool's methods, and scores them against
//! what was really there:
//!
//! * **error** -- mean absolute difference from the original, per
//!   channel, over the fully replaced pixels;
//! * **texture** -- the fill's mean gradient over the original's, inside
//!   the hole. 1.0 keeps the detail; a smear scores near 0.
//!
//! Error alone favours a blur (it is the least-squares answer), which is
//! why texture is reported beside it.
//!
//! ```sh
//! # binary PPMs (P6), e.g. the Kodak set converted with PIL
//! cargo run --release -p schist-tools-retouch --example remove_eval -- /tmp/kodak/*.ppm
//! ```

use std::collections::BTreeMap;
use std::time::Instant;

use schist_color::Rgba;
use schist_core::IntRect;
use schist_plugin_api::JobControl;
use schist_tools_retouch::remove::{
    context_window, fill_hole, removal_mask, Dab, Method, SPOT_HEAL_MAX_THICKNESS,
};

fn read_ppm(path: &str) -> (Vec<Rgba>, usize, usize) {
    let bytes = std::fs::read(path).expect("read");
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let start = i;
        while !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(String::from_utf8_lossy(&bytes[start..i]).to_string());
    }
    assert_eq!(fields[0], "P6", "{path}: binary PPM only");
    let (w, h): (usize, usize) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    let px = &bytes[i + 1..i + 1 + w * h * 3];
    let rgba = px
        .as_chunks::<3>()
        .0
        .iter()
        .map(|c| {
            Rgba::new(
                c[0] as f32 / 255.0,
                c[1] as f32 / 255.0,
                c[2] as f32 / 255.0,
                1.0,
            )
        })
        .collect();
    (rgba, w, h)
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// A wandering stroke of constant radius, the shape a wire or a scratch
/// is painted with.
fn stroke(rng: &mut Rng, w: usize, h: usize, r: f32) -> Vec<Dab> {
    let mut x = 80.0 + rng.next() * (w as f32 - 160.0);
    let mut y = 80.0 + rng.next() * (h as f32 - 160.0);
    let mut angle = rng.next() * std::f32::consts::TAU;
    let mut out = vec![Dab { x, y, r }];
    for _ in 0..60 {
        angle += (rng.next() - 0.5) * 0.5;
        x = (x + angle.cos() * 1.5).clamp(40.0, w as f32 - 40.0);
        y = (y + angle.sin() * 1.5).clamp(40.0, h as f32 - 40.0);
        out.push(Dab { x, y, r });
    }
    out
}

#[derive(Default)]
struct Score {
    n: usize,
    error: f64,
    texture: f64,
    seconds: f64,
}

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    assert!(!paths.is_empty(), "usage: remove_eval <image.ppm>...");
    let model = schist_neural::get("inpaint");
    if model.is_none() {
        eprintln!("inpainting model unavailable: Fill is the classical path");
    }
    let mut rng = Rng(0x5eed);
    // thickness bucket -> (spot, fill)
    let mut table: BTreeMap<u32, (Score, Score)> = BTreeMap::new();
    for path in &paths {
        let (truth, w, h) = read_ppm(path);
        let canvas = IntRect::new(0, 0, w as i32, h as i32);
        for &r in &[0.75f32, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 9.0] {
            for _ in 0..3 {
                let s = stroke(&mut rng, w, h, r);
                let Some(mask) = removal_mask(&[s], canvas) else {
                    continue;
                };
                let win = context_window(mask.rect, canvas);
                let (ww, wh) = (win.width() as usize, win.height() as usize);
                let mut pixels = Vec::with_capacity(ww * wh);
                let mut hole = vec![false; ww * wh];
                let mut full = vec![false; ww * wh];
                for y in 0..wh {
                    for x in 0..ww {
                        let (gx, gy) = (win.left as usize + x, win.top as usize + y);
                        pixels.push(truth[gy * w + gx]);
                        let (mx, my) = (gx as i32 - mask.rect.left, gy as i32 - mask.rect.top);
                        if mx >= 0 && my >= 0 && mx < mask.rect.width() && my < mask.rect.height() {
                            let i = my as usize * mask.rect.width() as usize + mx as usize;
                            hole[y * ww + x] = mask.hole[i];
                            full[y * ww + x] = mask.weight[i] >= 1.0;
                        }
                    }
                }
                let bucket = mask.thickness.round() as u32;
                let entry = table.entry(bucket).or_default();
                for (method, score) in [
                    (Method::SpotHeal, &mut entry.0),
                    (Method::Fill, &mut entry.1),
                ] {
                    let started = Instant::now();
                    let fill = fill_hole(
                        method,
                        pixels.clone(),
                        ww,
                        wh,
                        &hole,
                        model.as_deref(),
                        &JobControl::new(),
                    )
                    .unwrap();
                    score.seconds += started.elapsed().as_secs_f64();
                    let (mut err, mut n) = (0.0f64, 0usize);
                    let (mut gf, mut gt) = (0.0f64, 0.0f64);
                    for y in 0..wh - 1 {
                        for x in 0..ww - 1 {
                            let i = y * ww + x;
                            if !full[i] {
                                continue;
                            }
                            let (a, b) = (fill[i], pixels[i]);
                            err += ((a.r - b.r).abs() + (a.g - b.g).abs() + (a.b - b.b).abs())
                                as f64
                                / 3.0;
                            n += 1;
                            let lum = |p: Rgba| 0.299 * p.r + 0.587 * p.g + 0.114 * p.b;
                            gf += ((lum(fill[i + 1]) - lum(a)).abs()
                                + (lum(fill[i + ww]) - lum(a)).abs())
                                as f64;
                            gt += ((lum(pixels[i + 1]) - lum(b)).abs()
                                + (lum(pixels[i + ww]) - lum(b)).abs())
                                as f64;
                        }
                    }
                    if n > 0 && gt > 0.0 {
                        score.n += 1;
                        score.error += err / n as f64;
                        score.texture += gf / gt;
                    }
                }
            }
        }
    }
    println!(
        "thickness  n   spot err  fill err   spot tex  fill tex   spot s  fill s   (threshold {SPOT_HEAL_MAX_THICKNESS})"
    );
    for (t, (spot, fill)) in &table {
        let k = spot.n.max(1) as f64;
        println!(
            "{t:>9} {:>3}   {:>7.4}   {:>7.4}   {:>7.3}   {:>7.3}   {:>6.3}  {:>6.3}",
            spot.n,
            spot.error / k,
            fill.error / k,
            spot.texture / k,
            fill.texture / k,
            spot.seconds / k,
            fill.seconds / k,
        );
    }
}
