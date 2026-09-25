import random
import tempfile
import unittest
from pathlib import Path

import numpy as np
from PIL import Image
import torch

from anti_smudge import AntiSmudgeNet, CONTEXT, OVERLAP, Corpus, check_split, compose, export, loss_fn
from mfdnet_transfer import Restoration
from small_halos import add_small_halos, halo_error


torch.set_num_threads(2)


class AntiSmudgeTests(unittest.TestCase):
    def test_small_halo_pairs_keep_sources_and_add_only_positive_scatter(self):
        scene = torch.full((3, 64, 64), .12)
        bad, good = add_small_halos(scene, random.Random(5))
        again, _ = add_small_halos(scene, random.Random(5))
        torch.testing.assert_close(bad, again)
        self.assertTrue(torch.all(bad >= good))
        self.assertTrue(torch.all(good >= scene - 1e-6))
        self.assertGreater(float(good.max()), .99)
        self.assertGreater(int(((bad - good).mean(0) > .025).sum()), 50)
        saturated = good == 1
        self.assertTrue(saturated.any())
        torch.testing.assert_close(bad[saturated], good[saturated])
        self.assertTrue(torch.isfinite(bad).all())
        self.assertLessEqual(float(bad.max()), 1)
        self.assertGreater(float(halo_error(bad[None], good[None], bad[None])), 0)
        self.assertEqual(float(halo_error(good[None], good[None], bad[None])), 0)
        self.assertEqual(float(halo_error(good[None], good[None], good[None])), 0)

    def test_tighter_light_recovery_reduces_halo_and_preserves_bright_core(self):
        class RemoveScatter(torch.nn.Module):
            def forward(self, image):
                return image - .2

        # An underexposed source still needs protection even below white.
        image = torch.tensor([.3, .575, .71]).reshape(1, 1, 1, 3).expand(1, 3, 1, 3)
        previous = Restoration(RemoveScatter())(image)
        updated = Restoration(RemoveScatter(), .55, .70)(image)
        torch.testing.assert_close(updated[..., 0], image[..., 0] - .2)
        self.assertTrue(torch.all(updated[..., 1] < previous[..., 1]))
        torch.testing.assert_close(updated[..., 2], image[..., 2])
        self.assertTrue(torch.isfinite(updated).all())

    def test_scatter_is_added_and_true_light_is_retained(self):
        scene = np.full((65, 65, 3), .08, dtype=np.float32)
        flare = np.zeros_like(scene)
        flare[32, :, :] = .25
        flare[32, 32, :] = 1
        bad, target = compose(scene, flare)
        self.assertGreater(float(bad[32, 8, 0]), float(target[32, 8, 0]) + .1)
        np.testing.assert_allclose(target[32, 32], bad[32, 32], atol=1e-6)
        np.testing.assert_allclose(target[0, 0], scene[0, 0], atol=1e-6)
        self.assertTrue(np.isfinite(bad).all())

    def test_annotated_light_target_is_preserved(self):
        scene = np.full((16, 16, 3), .1, dtype=np.float32)
        flare = np.full_like(scene, .3)
        source = np.zeros_like(scene)
        source[7:9, 7:9] = .3
        bad, target = compose(scene, flare, source)
        np.testing.assert_allclose(bad[8, 8], target[8, 8])
        np.testing.assert_allclose(target[0, 0], scene[0, 0], atol=1e-6)

    def test_model_starts_at_identity_and_can_learn(self):
        torch.manual_seed(3)
        model = AntiSmudgeNet(4)
        image = torch.rand(1, 3, 24, 24)
        torch.testing.assert_close(model(image), image)
        optimizer = torch.optim.Adam(model.parameters(), lr=.001)
        before = loss_fn(model(image), image * .8)
        optimizer.zero_grad()
        before.backward()
        optimizer.step()
        self.assertFalse(torch.equal(model(image), image))
        self.assertLessEqual(CONTEXT, OVERLAP)

    def test_duplicate_content_is_rejected_even_after_rename(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for split, name in [("train", "a.png"), ("val", "b.png")]:
                for kind in ["input", "target"]:
                    p = root / split / kind / name
                    p.parent.mkdir(parents=True)
                    Image.new("RGB", (16, 16), "red").save(p)
            with self.assertRaisesRegex(ValueError, "leakage"):
                check_split(Corpus(root / "train", "paired"), Corpus(root / "val", "paired"))

    def test_pairs_remain_aligned_and_controls_are_identity(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            image = np.random.default_rng(1).integers(0, 128, (17, 19, 3), dtype=np.uint8)
            for kind, pixels in [("input", image + 30), ("target", image)]:
                p = root / kind / "a.png"
                p.parent.mkdir()
                Image.fromarray(pixels).save(p)
            corpus = Corpus(root, "paired")
            bad, good = corpus.sample(random.Random(1), 16)
            torch.testing.assert_close(bad - good, torch.full_like(good, 30 / 255))
            bad, good = corpus.sample(random.Random(1), 16, identity_probability=1)
            torch.testing.assert_close(bad, good)

    def test_onnx_matches_pytorch_with_nonzero_weights(self):
        from onnx.reference import ReferenceEvaluator
        torch.manual_seed(1)
        for architecture in ("dilated", "residual", "pyramid"):
            with self.subTest(architecture=architecture):
                model = AntiSmudgeNet(4, architecture).eval()
                torch.nn.init.normal_(model.tail.weight, std=.002)
                sample = torch.rand(1, 3, 21, 25)
                with tempfile.TemporaryDirectory() as tmp:
                    path = Path(tmp) / "test.onnx"
                    export(model, path)
                    output = ReferenceEvaluator(str(path)).run(None, {"input": sample.numpy()})[0]
                np.testing.assert_allclose(output, model(sample).detach().numpy(), atol=2e-5, rtol=2e-5)

    def test_pyramid_context_and_stride_do_not_create_tile_seams(self):
        torch.manual_seed(9)
        model = AntiSmudgeNet(4, "pyramid").eval()
        torch.nn.init.normal_(model.tail.weight, std=.1)
        image = torch.rand(1, 3, 384, 576)
        with torch.no_grad():
            whole = model(image)
            for start in (0, 192):
                tile = model(image[..., start:start+384])
                torch.testing.assert_close(tile[..., 96:288, 96:288],
                                           whole[..., 96:288, start+96:start+288],
                                           atol=2e-5, rtol=2e-5)


if __name__ == "__main__":
    unittest.main()
