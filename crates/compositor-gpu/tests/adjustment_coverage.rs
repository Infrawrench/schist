#![cfg(not(target_arch = "wasm32"))]
use schist_adjustments::{HueRange, Params};
use schist_color::{Depth, Rgba};
use schist_compositor::{Compositor, CpuCompositor};
use schist_compositor_gpu::{plan, BatchOut, GpuContext};
use schist_core::{AdjustmentData, Document, Layer, LayerKind, TileCoord};

#[test]
fn every_direct_adjustment_runs_on_gpu_with_variable_records() {
    let ctx = GpuContext::new().expect("GPU coverage tests require an adapter");
    let mut cases = Vec::new();
    for preserve in [false, true] {
        cases.push(Params::ColorBalance {
            shadows: [20.0, -31.0, 12.0],
            midtones: [-45.0, 21.0, 4.0],
            highlights: [16.0, 27.0, -33.0],
            preserve_luminosity: preserve,
        });
        cases.push(Params::PhotoFilter {
            color: [0.929, 0.510, 0.208],
            density: 67.0,
            preserve_luminosity: preserve,
        });
        cases.push(Params::ChannelMixer {
            red: [71.0, 29.0, -17.0],
            green: [19.0, 62.0, 13.0],
            blue: [-20.0, 35.0, 85.0],
            constant: [5.0, -3.0, 12.0],
            monochrome: preserve,
        });
        cases.push(Params::SelectiveColor {
            ranges: [
                [12.0, -15.0, 8.0, -9.0],
                [-8.0, 23.0, 11.0, 7.0],
                [17.0, 8.0, -19.0, 20.0],
                [-11.0, 4.0, 17.0, -6.0],
                [31.0, -18.0, 3.0, 12.0],
                [-7.0, 21.0, 15.0, -18.0],
            ],
            relative: preserve,
        });
        for stops in [
            vec![],
            vec![
                (0.0, [0.2, 0.1, 0.5]),
                (0.37, [0.9, 0.2, 0.3]),
                (0.61, [0.1, 0.7, 0.6]),
                (1.0, [0.8, 0.9, 0.4]),
            ],
        ] {
            cases.push(Params::GradientMap {
                from: [0.1, 0.3, 0.7],
                to: [0.9, 0.6, 0.2],
                reverse: preserve,
                stops,
            });
        }
        cases.push(Params::HueSaturation {
            hue: -23.0,
            saturation: 17.0,
            lightness: -11.0,
            colorize: preserve,
            lightness_desaturates: true,
            reciprocal_saturation: true,
            ranges: vec![
                HueRange {
                    bounds: [315.0, 345.0, 15.0, 45.0],
                    hue: 35.0,
                    saturation: -30.0,
                    lightness: 20.0,
                },
                HueRange {
                    bounds: [50.0, 90.0, 145.0, 195.0],
                    hue: -45.0,
                    saturation: 25.0,
                    lightness: -35.0,
                },
            ],
        });
    }
    for (vibrance, saturation) in [(100.0, 0.0), (45.0, 25.0), (-100.0, -17.0), (0.0, 37.0)] {
        cases.push(Params::Vibrance {
            vibrance,
            saturation,
        });
    }
    for (warmth, tint) in [
        (0.0, 0.0),
        (-100.0, -100.0),
        (100.0, 100.0),
        (37.0, -61.0),
        (-45.0, 25.0),
    ] {
        cases.push(Params::WhiteBalance { warmth, tint });
    }
    let coord = TileCoord { tx: 0, ty: 0 };
    for params in cases {
        let mut doc = Document::new("adjustments", 37, 29, Depth::ThirtyTwo);
        let mut layer = Layer::new_raster("pixels");
        let tile = layer
            .as_raster_mut()
            .unwrap()
            .tiles
            .get_mut_or_insert(coord, doc.depth);
        for y in 0..29 {
            for x in 0..37 {
                let i = y * 37 + x;
                tile.set(
                    y * 256 + x,
                    Rgba::new(
                        (i % 31) as f32 / 30.0,
                        (i % 23) as f32 / 22.0,
                        (i % 17) as f32 / 16.0,
                        if i % 11 == 0 { 0.0 } else { 0.73 },
                    ),
                );
            }
        }
        doc.tree.layers.push(layer);
        let mut adjustment = Layer::new_raster("adjustment");
        adjustment.kind = LayerKind::Adjustment(AdjustmentData {
            kind: params.kind(),
            raw: vec![],
            params_json: Some(serde_json::to_string(&params).unwrap()),
        });
        doc.tree.layers.push(adjustment);
        let plan = plan::build(&doc).expect("every supported adjustment must compile");
        let Some(BatchOut::F32(actual)) = ctx.composite_batch(&plan, &[coord], false) else {
            panic!("GPU declined {params:?}")
        };
        let expected = CpuCompositor.tile(&doc, coord);
        let worst = actual[0]
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < 0.0002, "{params:?}: maximum error {worst}");
    }
}
