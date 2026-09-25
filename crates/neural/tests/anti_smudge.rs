use schist_neural::{self as neural, Model, ModelSource};

const SCALE: &[u8] = include_bytes!("fixtures/anti-smudge-scale.onnx");
const DIVIDE: &[u8] = include_bytes!("fixtures/anti-smudge-divide.onnx");
const COARSE_SCALE: &[u8] = include_bytes!("fixtures/anti-smudge-coarse-scale.onnx");
const COARSE_DIVIDE: &[u8] = include_bytes!("fixtures/anti-smudge-coarse-divide.onnx");

// Arithmetic fixtures without metadata retain their original 384px contract.
fn fixture_spec() -> &'static neural::ModelSpec {
    static SPEC: std::sync::OnceLock<neural::ModelSpec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| {
        let mut spec = neural::spec("anti-smudge").unwrap().clone();
        spec.source = ModelSource::Local;
        spec.sha256 = None;
        spec.input = neural::Input::Tiles {
            size: 384,
            overlap: 96,
            scale: 1,
        };
        spec
    })
}

#[test]
fn anti_smudge_honors_bounded_model_tile_dimensions() {
    let model = Model::from_bytes(
        fixture_spec(),
        include_bytes!("fixtures/anti-smudge-tile64.onnx"),
    )
    .unwrap();
    let mut rgb = vec![0.5; 80 * 70 * 3];
    neural::try_restore(&model, &mut rgb, 80, 70, 1.0).unwrap();
    assert!(rgb.iter().all(|v| (*v - 0.25).abs() < 1e-6));
    assert!(Model::from_bytes(
        fixture_spec(),
        include_bytes!("fixtures/anti-smudge-invalid-tile.onnx")
    )
    .is_err());
    let large = Model::from_bytes(
        fixture_spec(),
        include_bytes!("fixtures/anti-smudge-tile2048.onnx"),
    )
    .unwrap();
    let mut pixel = [0.3, 0.4, 0.5];
    neural::try_restore(&large, &mut pixel, 1, 1, 1.0).unwrap();
    assert_eq!(pixel, [0.3, 0.4, 0.5]);
    assert!(Model::from_bytes(
        fixture_spec(),
        include_bytes!("fixtures/anti-smudge-oversized-tile.onnx")
    )
    .is_err());
}

#[test]
fn anti_smudge_optional_halo_cleanup_honors_strength() {
    let model = Model::from_bytes(
        fixture_spec(),
        include_bytes!("fixtures/anti-smudge-halo.onnx"),
    )
    .unwrap();
    let source: Vec<f32> = (0..128 * 128)
        .flat_map(|i| {
            let r2 = ((i % 128) as f32 - 64.0).powi(2) + ((i / 128) as f32 - 64.0).powi(2);
            if r2 <= 9.0 {
                [0.8; 3]
            } else {
                [0.25, 0.16, 0.02].map(|a| 0.17 + a * (-r2 / 200.0).exp())
            }
        })
        .collect();
    let mut full = source.clone();
    neural::try_restore(&model, &mut full, 128, 128, 1.0).unwrap();
    assert!(full[(64 * 128 + 79) * 3] < source[(64 * 128 + 79) * 3] - 0.05);
    assert_eq!(&full[(64 * 128 + 64) * 3..][..3], &[0.8; 3]);
    let mut partial = source.clone();
    neural::try_restore(&model, &mut partial, 128, 128, 0.4).unwrap();
    for ((p, s), f) in partial.iter().zip(&source).zip(&full) {
        assert!((*p - (*s + (*f - *s) * 0.4)).abs() < 1e-6);
    }
    let mut zero = source.clone();
    neural::try_restore(&model, &mut zero, 128, 128, 0.0).unwrap();
    assert_eq!(zero, source);
    assert!(Model::from_bytes(
        fixture_spec(),
        include_bytes!("fixtures/anti-smudge-invalid-halo.onnx")
    )
    .is_err());
}

#[test]
fn anti_smudge_coarse_correction_preserves_fine_detail() {
    let model = Model::from_bytes(fixture_spec(), COARSE_SCALE).unwrap();
    let mut rgb: Vec<f32> = (0..64 * 64)
        .flat_map(|i| [if (i % 64 + i / 64) % 2 == 0 { 0.3 } else { 0.7 }; 3])
        .collect();
    let original = rgb.clone();
    neural::try_restore(&model, &mut rgb, 64, 64, 0.6).unwrap();
    // Each 2x2 average is 0.5; the learned -0.25 correction becomes -0.15
    // at this strength. Original checkerboard contrast must remain intact.
    for (actual, input) in rgb.iter().zip(&original) {
        assert!((actual - (input - 0.15)).abs() < 1e-6);
    }
    for (w, h) in [(65, 1), (1, 65)] {
        let mut thin = vec![0.5; w * h * 3];
        neural::try_restore(&model, &mut thin, w, h, 1.0).unwrap();
        assert!(thin.iter().all(|v| (*v - 0.25).abs() < 1e-6));
    }
}

#[test]
fn anti_smudge_coarse_failure_is_atomic() {
    let model = Model::from_bytes(fixture_spec(), COARSE_DIVIDE).unwrap();
    let mut rgb = vec![0.5; 64 * 64 * 3];
    for y in 0..4 {
        rgb[y * 64 * 3..y * 64 * 3 + 12].fill(0.0);
    }
    let original = rgb.clone();
    assert!(neural::try_restore(&model, &mut rgb, 64, 64, 1.0).is_err());
    assert_eq!(rgb, original);
    neural::try_restore(&model, &mut rgb, 64, 64, 0.0).unwrap();
    assert_eq!(rgb, original);
}

#[test]
fn anti_smudge_is_bundled_and_needs_no_external_download() {
    let spec = neural::spec("anti-smudge").unwrap();
    assert_eq!(spec.source, ModelSource::BuiltIn);
    assert!(spec.built_in());
    assert!(spec.file.ends_with(".onnx.xz"));
    assert!(spec.sha256.is_some());
    assert!(neural::download_url(spec).is_none());
    assert_eq!(spec.input.dims(), (2048, 2048));
}

#[test]
fn anti_smudge_blends_across_tile_seams_and_handles_thin_images() {
    let model = Model::from_bytes(fixture_spec(), SCALE).unwrap();
    for (w, h) in [(401, 199), (1, 1), (1, 3), (4, 1)] {
        let original: Vec<_> = (0..w * h * 3).map(|i| (i % 97) as f32 / 96.0).collect();
        let mut rgb = original.clone();
        neural::try_run_tiled(&model, &mut rgb, w, h, 0.6).unwrap();
        for (actual, input) in rgb.iter().zip(&original) {
            assert!((actual - input * 0.7).abs() < 1e-6);
        }
        neural::try_run_tiled(&model, &mut rgb, w, h, 0.0).unwrap();
    }
}

#[test]
fn anti_smudge_failed_later_tile_does_not_commit_earlier_tiles() {
    let model = Model::from_bytes(fixture_spec(), DIVIDE).unwrap();
    let (w, h) = (401, 1);
    let mut rgb = vec![0.5; w * h * 3];
    rgb[(w - 1) * 3] = 0.0; // Only the later tiles see 0/0.
    let original = rgb.clone();
    assert!(neural::try_run_tiled(&model, &mut rgb, w, h, 1.0).is_err());
    assert_eq!(rgb, original);
}

#[test]
fn anti_smudge_rejects_bad_buffers_and_invalid_imports() {
    let spec = fixture_spec();
    let model = Model::from_bytes(spec, SCALE).unwrap();
    let mut rgb = vec![0.5; 3];
    assert!(neural::try_run_tiled(&model, &mut rgb, usize::MAX, 2, 1.0).is_err());
    assert!(neural::try_run_tiled(&model, &mut rgb, 1, 1, f32::NAN).is_err());
    assert!(neural::install_local(spec, b"not ONNX").is_err());
    assert!(neural::install_local(neural::spec("detail").unwrap(), SCALE).is_err());
    assert_eq!(rgb, vec![0.5; 3]);
}
