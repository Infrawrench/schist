use super::*;

fn scene(width: u32, height: u32, ox: i32, oy: i32) -> Image {
    let mut rgba = Vec::new();
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let (x, y) = ((x + ox) as f32, (y + oy) as f32);
            let v = 0.5
                + 0.13 * (x * 0.13 + y * 0.09).sin()
                + 0.11 * (x * 0.19 - y * 0.17).cos()
                + 0.12 * (x * 0.037 + y * 0.053).sin()
                + 0.09 * (x * 0.071 - y * 0.091).cos();
            rgba.extend_from_slice(&[v, v * 0.8, v * 0.7, 1.0]);
        }
    }
    Image {
        width,
        height,
        rgba,
    }
}

#[test]
fn registers_positive_and_negative_translation_with_different_dimensions_and_exposure() {
    let a = scene(180, 145, 0, 0);
    let mut b = scene(162, 138, 17, -9);
    for p in b.rgba.chunks_exact_mut(4) {
        for v in &mut p[..3] {
            *v = *v * 0.63 + 0.07;
        }
    }
    assert_eq!(
        register(&a, &b, false, &Control::default()).unwrap().0,
        Offset { x: 17, y: -9 }
    );
    let out = merge(
        &[a, b],
        &Options {
            crop: true,
            ..Default::default()
        },
        &Control::default(),
    )
    .unwrap();
    assert_eq!((out.width, out.height), (162, 129));
    assert_eq!(
        out.offsets,
        [Offset { x: -17, y: 0 }, Offset { x: 0, y: -9 }]
    );
    assert!(out.merged.is_none());
}

#[test]
fn panorama_registers_sequential_overlap_and_reconstructs_scene() {
    let a = scene(150, 110, 0, 0);
    let b = scene(150, 110, 90, 3);
    let c = scene(150, 110, 179, -2);
    let output = merge(
        &[a, b, c],
        &Options {
            mode: Mode::Panorama,
            ..Default::default()
        },
        &Control::default(),
    )
    .unwrap();
    assert_eq!((output.width, output.height), (329, 115));
    assert_eq!(output.offsets[2], Offset { x: 179, y: 0 });
    let result = output.merged.unwrap();
    let expected = scene(329, 115, 0, -2);
    for (got, want) in result
        .rgba
        .chunks_exact(4)
        .zip(expected.rgba.chunks_exact(4))
    {
        if got[3] > 0.0 {
            for c in 0..3 {
                assert!((got[c] - want[c]).abs() < 1e-5);
            }
        }
    }
    assert_eq!(result.at(0, 0).unwrap()[3], 0.0);
}

#[test]
fn focus_selects_sharp_regions_from_different_sources() {
    let mut a = scene(100, 64, 0, 0);
    let mut b = a.clone();
    for y in 0..64usize {
        for x in 0..100usize {
            let detail = if (x + y) % 2 == 0 { 0.25 } else { 0.75 };
            let i = (y * 100 + x) * 4;
            for c in 0..3 {
                a.rgba[i + c] = if x < 50 { detail } else { 0.5 };
                b.rgba[i + c] = if x >= 50 { detail } else { 0.5 };
            }
        }
    }
    let output = merge(
        &[a, b],
        &Options {
            mode: Mode::Focus,
            align: false,
            ..Default::default()
        },
        &Control::default(),
    )
    .unwrap()
    .merged
    .unwrap();
    for y in 5..59usize {
        for x in (5..40usize).chain(60..95usize) {
            let expected = if (x + y) % 2 == 0 { 0.25 } else { 0.75 };
            assert_eq!(output.rgba[(y * 100 + x) * 4], expected);
        }
    }
}

#[test]
fn hdr_recovers_clipped_highlights_using_supplied_exposures() {
    let exposures = [-3.0f32, 0.0, 3.0];
    let radiances = [0.005f32, 0.08, 0.4, 2.0, 6.0];
    let images = exposures
        .iter()
        .map(|ev| {
            let mut rgba = Vec::new();
            for r in radiances {
                let p = linear_to_srgb((r * ev.exp2()).min(1.0));
                rgba.extend_from_slice(&[p, p, p, 1.0]);
            }
            Image {
                width: 5,
                height: 1,
                rgba,
            }
        })
        .collect::<Vec<_>>();
    let output = merge(
        &images,
        &Options {
            mode: Mode::Hdr,
            align: false,
            exposure_ev: exposures.to_vec(),
            ..Default::default()
        },
        &Control::default(),
    )
    .unwrap()
    .merged
    .unwrap();
    for (i, radiance) in radiances.iter().enumerate() {
        let expected = linear_to_srgb(*radiance / (1.0 + radiance));
        assert!((output.rgba[i * 4] - expected).abs() < 1e-5);
    }
    assert!(output.rgba[16] > output.rgba[12]);
    let hdr = merge(
        &images,
        &Options {
            mode: Mode::Hdr,
            align: false,
            tone_map: false,
            exposure_ev: exposures.to_vec(),
            ..Default::default()
        },
        &Control::default(),
    )
    .unwrap()
    .merged
    .unwrap();
    for (i, radiance) in radiances.iter().enumerate() {
        assert!((srgb_to_linear(hdr.rgba[i * 4]) - radiance).abs() < 1e-5);
    }
    assert!(hdr.rgba[16] > 1.0);
}

#[test]
fn tone_mapping_preserves_linear_chromaticity() {
    let got = tone_map([4.0, 2.0, 1.0], -1.0);
    assert!((got[0] / got[1] - 2.0).abs() < 1e-6);
    assert!((got[1] / got[2] - 2.0).abs() < 1e-6);
    assert!(luma(&got) < 1.0);
}

#[test]
fn rejects_blank_unrelated_and_invalid_inputs_and_honors_cancellation() {
    let a = scene(90, 70, 0, 0);
    let mut b = a.clone();
    b.rgba.fill(0.5);
    assert_eq!(
        register(&a, &b, true, &Control::default()),
        Err(Error::NoMatch)
    );
    let mut seed = 12345u32;
    for v in &mut b.rgba {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *v = (seed & 65535) as f32 / 65535.0;
    }
    assert_eq!(
        register(&a, &b, true, &Control::default()),
        Err(Error::NoMatch)
    );
    assert_eq!(pixels(u32::MAX, 2), Err(Error::TooLarge));
    b.rgba[0] = f32::NAN;
    assert_eq!(b.validate(), Err(Error::Invalid));
    let control = Control::default();
    control.cancelled.store(true, Ordering::Relaxed);
    assert_eq!(
        merge(&[a.clone(), a], &Options::default(), &control).unwrap_err(),
        Error::Cancelled
    );
}

#[test]
fn transparent_inputs_do_not_contribute_to_panorama_or_hdr() {
    let mut a = scene(20, 10, 0, 0);
    let mut b = a.clone();
    for p in a.rgba.chunks_exact_mut(4) {
        p[3] = 0.0;
    }
    for p in b.rgba.chunks_exact_mut(4) {
        p[3] = 0.5;
    }
    let result = merge(
        &[a, b.clone()],
        &Options {
            mode: Mode::Hdr,
            align: false,
            exposure_ev: vec![0.0, 0.0],
            ..Default::default()
        },
        &Control::default(),
    )
    .unwrap()
    .merged
    .unwrap();
    assert_eq!(result.rgba[3], 0.5);
    assert!(result.rgba.iter().all(|v| v.is_finite()));
    let expected = tone_map(
        [
            srgb_to_linear(b.rgba[0]),
            srgb_to_linear(b.rgba[1]),
            srgb_to_linear(b.rgba[2]),
        ],
        0.0,
    );
    assert!((result.rgba[0] - linear_to_srgb(expected[0])).abs() < 1e-6);
}

#[test]
fn panorama_feathers_disagreement_in_linear_light() {
    let a = scene(100, 70, 0, 0);
    let mut b = scene(100, 70, 30, 0);
    for p in b.rgba.chunks_exact_mut(4) {
        for v in &mut p[..3] {
            *v *= 0.8;
        }
    }
    let result = merge(
        &[a.clone(), b.clone()],
        &Options {
            mode: Mode::Panorama,
            feather: 20,
            ..Default::default()
        },
        &Control::default(),
    )
    .unwrap()
    .merged
    .unwrap();
    let (x, y) = (35, 35);
    let av = a.at(x, y).unwrap()[0];
    let bv = b.at(x - 30, y).unwrap()[0];
    let w = 6.0 / 20.0;
    let expected = linear_to_srgb((srgb_to_linear(av) + srgb_to_linear(bv) * w) / (1.0 + w));
    assert!((result.at(x, y).unwrap()[0] - expected).abs() < 1e-6);
    assert!((result.at(15, y).unwrap()[0] - a.at(15, y).unwrap()[0]).abs() < 1e-6);
}

#[test]
fn flat_focus_ties_keep_the_more_opaque_sample() {
    let a = Image {
        width: 8,
        height: 8,
        rgba: [0.3, 0.3, 0.3, 0.2].repeat(64),
    };
    let b = Image {
        width: 8,
        height: 8,
        rgba: [0.3, 0.3, 0.3, 1.0].repeat(64),
    };
    let result = merge(
        &[a, b],
        &Options {
            mode: Mode::Focus,
            align: false,
            ..Default::default()
        },
        &Control::default(),
    )
    .unwrap()
    .merged
    .unwrap();
    assert!(result.rgba.chunks_exact(4).all(|p| p[3] == 1.0));
}

#[test]
fn checks_budget_shape_and_options_before_work() {
    let bad_size = Image {
        width: 30_001,
        height: 1,
        rgba: Vec::new(),
    };
    assert_eq!(bad_size.validate(), Err(Error::TooLarge));
    let bad_buffer = Image {
        width: 3,
        height: 2,
        rgba: vec![0.0; 4],
    };
    assert_eq!(bad_buffer.validate(), Err(Error::Invalid));
    let a = scene(8, 8, 0, 0);
    assert_eq!(
        merge(&[a.clone()], &Options::default(), &Control::default()).unwrap_err(),
        Error::Invalid
    );
    assert_eq!(
        merge(
            &vec![a.clone(); MAX_IMAGES + 1],
            &Options::default(),
            &Control::default()
        )
        .unwrap_err(),
        Error::Invalid
    );
    assert_eq!(
        merge(
            &[a.clone(), a.clone()],
            &Options {
                mode: Mode::Hdr,
                exposure_ev: vec![f32::NAN, 0.0],
                ..Default::default()
            },
            &Control::default()
        )
        .unwrap_err(),
        Error::Invalid
    );
    // The alignment workflow always registers, even after an earlier focus/HDR
    // workflow disabled its optional registration checkbox.
    let other = scene(80, 70, -7, 9);
    let first = scene(80, 70, 0, 0);
    let result = merge(
        &[first, other],
        &Options {
            align: false,
            ..Default::default()
        },
        &Control::default(),
    )
    .unwrap();
    assert_eq!(result.offsets[1].y - result.offsets[0].y, 9);
    assert_eq!(result.offsets[1].x - result.offsets[0].x, -7);
}
