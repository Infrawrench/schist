"""Small regression checks for camera decoding and detector contracts."""

from pathlib import Path
import tempfile
import unittest

import numpy as np
from PIL import Image

from remove_background import SRGB_PROFILE, detect, load_rgba, refine, session, resize_antialiased


class FakeDetector:
    def __init__(self, output):
        self.output = output
        self.input = None

    def get_inputs(self):
        return [type("Input", (), {"name": "rgb"})()]

    def run(self, _, values):
        self.input = values["rgb"]
        return [self.output]


class BackgroundTests(unittest.TestCase):
    def test_detail_windows_reconstruct_soft_strands_without_joins(self):
        from detail_matting import refine_detail
        class Identity:
            def get_inputs(self):
                return [type("Input", (), {"name": "rgb_trimap"})()]
            def run(self, _, values):
                return [values["rgb_trimap"][:, :1]]
        x = np.arange(901, dtype=np.float32)
        alpha = np.full((27, len(x)), .5, dtype=np.float32)
        expected = np.broadcast_to(np.sin(x*.1)*.4+.5, alpha.shape)
        rgb = np.stack([expected, expected*.7, expected*.3], axis=-1)
        np.testing.assert_allclose(refine_detail(Identity(), rgb, alpha), expected, atol=2e-7)
        for a in [0., 1.]:
            coarse = np.full_like(alpha, a)
            np.testing.assert_array_equal(refine_detail(None, rgb, coarse), coarse)

    def test_detail_trimap_keeps_gaps_and_unknown_hair(self):
        from detail_matting import trimap, refine_detail
        alpha = np.zeros((17, 41), np.float32)
        alpha[:, 20:] = 1
        alpha[:, 8] = .6
        tri = trimap(alpha)
        np.testing.assert_array_equal(tri[8, [0, 8, 16, 24]], [0, .5, .5, 1])
        with self.assertRaisesRegex(ValueError, "non-finite"):
            refine_detail(None, np.zeros((17, 41, 3)), alpha*np.nan)

    def test_opaque_core_hint_restores_solid_material_without_locking_webbing(self):
        from detail_matting import trimap
        coarse = np.full((81, 121), .5, np.float32)
        hint = np.zeros_like(coarse)
        hint[5:76, 60:115] = 1  # Broad solid sleeve.
        hint[20:28, 10:50] = 1  # Thin opaque clump: not a reliable core.
        tri = trimap(coarse, hint)
        self.assertEqual(tri[40, 86], 1)
        self.assertEqual(tri[24, 30], .5)
        self.assertTrue((hint[tri == 1] > .98).all())
        coarse[40, 86] = 0
        coarse[36:45, 82:91] = 0
        self.assertEqual(trimap(coarse, hint)[40, 86], 0)

    def test_detector_agreement_recovers_clothing_without_admitting_new_background(self):
        from subject_guidance import constrain_detail
        reference = np.zeros((520, 520), np.float32)
        reference[100:300, 180:280] = 1
        coarse = reference.copy()
        coarse[300:450, 180:280] = 1  # Missed clothing, semantically supported.
        coarse[100:300, 280:400] = 1  # Nearby background, weak semantic support.
        probability = np.zeros_like(reference)
        probability[100:450, 180:280] = .99
        probability[100:300, 280:400] = .3
        coarse[180:220, 230:240] = 0  # Genuine gap: never fill it.
        result = constrain_detail(coarse, reference, probability)
        self.assertEqual(result[380, 230], 1)
        self.assertEqual(result[200, 350], 0)
        self.assertEqual(result[200, 235], 0)
        np.testing.assert_array_equal(constrain_detail(coarse, reference, np.zeros_like(probability)), reference)

    def test_foreground_color_cleanup_preserves_coverage_and_has_no_block_seams(self):
        from foreground_color import clean_foreground, _estimate, fractional
        yy, xx = np.mgrid[:113, :601]
        alpha = ((xx.astype(np.float32)-250+np.sin(yy*.17)*4)/18).clip(0, 1).astype(np.float32)
        color = np.array([.8, .1, .3], np.float32)
        background = np.array([.1, .9, .2], np.float32)
        rgb = alpha[..., None]*color + (1-alpha[..., None])*background
        original_alpha = alpha.copy()
        actual = clean_foreground(rgb, alpha)
        expected = _estimate(rgb, alpha)
        active = fractional(alpha)
        np.testing.assert_array_equal(alpha, original_alpha)
        np.testing.assert_array_equal(actual[~active], rgb[~active])
        np.testing.assert_allclose(actual[active], expected[active], atol=1e-6)
        old_error = (np.abs(rgb-color)*alpha[..., None])[active].mean()
        new_error = (np.abs(actual-color)*alpha[..., None])[active].mean()
        self.assertLess(new_error, old_error*.6)
        with self.assertRaisesRegex(ValueError, "invalid foreground"):
            clean_foreground(rgb, np.full_like(alpha, np.nan))
        for certain in [0.0, .0001, .9999, 1.0]:
            np.testing.assert_array_equal(clean_foreground(rgb, np.full_like(alpha, certain)), rgb)

    def test_cropped_subject_survives_without_restoring_detached_border_objects(self):
        from subject_guidance import guide_alpha
        alpha = np.zeros((520, 520), np.float32)
        probability = alpha.copy()
        alpha[:260, 200:280] = 1
        probability[100:260, 200:280] = .99
        alpha[:50, 400:440] = 1
        alpha[150:180, 280:350] = 1
        result = guide_alpha(alpha, probability)
        self.assertEqual(result[20, 240], 1)
        self.assertEqual(result[20, 420], 0)
        self.assertEqual(result[165, 330], 0)
        self.assertTrue((result <= alpha).all())

    def test_semantic_confidence_cannot_fill_hair_gaps_or_thicken_transparent_edges(self):
        from subject_guidance import guide_alpha
        alpha = np.zeros((520, 520), np.float32)
        probability = alpha.copy()
        probability[100:400, 100:400] = .999
        alpha[100:400, 100:400] = .6
        alpha[100:400, 240:260] = 0
        result = guide_alpha(alpha, probability)
        self.assertEqual(result[250, 250], 0)
        self.assertEqual(result[250, 200], alpha[250, 200])
        self.assertTrue((result <= alpha).all())

    def test_antialiasing_averages_fine_camera_texture(self):
        yy, xx = np.mgrid[:60, :60]
        rgb = np.repeat(((xx + yy) % 2).astype(np.float32)[..., None], 3, axis=2)
        np.testing.assert_allclose(resize_antialiased(rgb, 5, 5), .5, atol=.001)

    def test_subject_guide_keeps_multiple_subjects_and_rejects_distractor(self):
        from subject_guidance import guide_alpha
        alpha = np.zeros((520, 520), np.float32)
        probability = alpha.copy()
        for x, supported in [(40, True), (230, True), (420, False)]:
            alpha[100:160, x:x+50] = 1
            if supported:
                probability[100:160, x:x+50] = .99
        result = guide_alpha(alpha, probability)
        self.assertEqual(result[130, 65], 1)
        self.assertEqual(result[130, 255], 1)
        self.assertEqual(result[130, 445], 0)
        np.testing.assert_array_equal(guide_alpha(alpha, np.zeros_like(probability)), alpha)
        with self.assertRaisesRegex(ValueError, "invalid semantic"):
            guide_alpha(alpha, np.full_like(probability, np.nan))

    def test_camera_orientation_and_existing_alpha_survive_srgb_loading(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "camera.png"
            pixels = np.array([[[240, 20, 80, 0], [30, 210, 90, 128]]], dtype=np.uint8)
            exif = Image.Exif()
            exif[274] = 6
            Image.fromarray(pixels).save(path, exif=exif, icc_profile=SRGB_PROFILE)
            original = path.read_bytes()
            actual = np.asarray(load_rgba(path))
            np.testing.assert_array_equal(actual, np.rot90(pixels, -1))
            self.assertEqual(path.read_bytes(), original)

    def test_birefnet_decodes_logits_without_image_peak_normalization(self):
        model = FakeDetector(np.array([[[[-80, 0], [80, 1]]]], np.float32))
        rgb = np.full((2, 2, 3), .2, dtype=np.float32)
        alpha = detect(model, rgb, "birefnet-lite")
        np.testing.assert_allclose(alpha, [[0, .5], [1, .7310586]], atol=1e-6)
        np.testing.assert_allclose(model.input[0, :, 0, 0],
            (np.array([.2, .2, .2]) - [.485, .456, .406]) / [.229, .224, .225], atol=1e-6)

    def test_nonfinite_detector_output_is_rejected(self):
        model = FakeDetector(np.full((1, 1, 2, 2), np.nan, np.float32))
        with self.assertRaisesRegex(ValueError, "non-finite"):
            detect(model, np.zeros((2, 2, 3), np.float32), "birefnet-lite")

    def test_refiner_cannot_damage_certain_pixels_inside_edge_tiles(self):
        rng = np.random.default_rng(32)
        rgb = rng.random((97, 193, 3), dtype=np.float32)
        coarse = np.broadcast_to(((np.arange(193, dtype=np.float32)-85)/20).clip(0, 1), (97, 193)).copy()
        model = session(Path(__file__).resolve().parents[2] / "crates/neural/models/matting.onnx.xz")
        actual = refine(model, rgb, coarse)
        certain = (coarse == 0) | (coarse == 1)
        np.testing.assert_array_equal(actual[certain], coarse[certain])


if __name__ == "__main__":
    unittest.main()
