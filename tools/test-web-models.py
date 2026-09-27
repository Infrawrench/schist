#!/usr/bin/env python3
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

module = importlib.util.spec_from_file_location("models", Path(__file__).with_name("web-models.py"))
models = importlib.util.module_from_spec(module)
module.loader.exec_module(models)


class ModelAssets(unittest.TestCase):
    def test_embedded_model_build_guard(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "test.onnx.xz").write_bytes(b"model payload")
            wasm = root / "app.wasm"
            wasm.write_bytes(b"code without weights")
            models.check_wasm(wasm, root)
            wasm.write_bytes(b"code model payload more code")
            with self.assertRaisesRegex(ValueError, "embedded"):
                models.check_wasm(wasm, root)

    def test_chunked_payload_round_trip_excludes_source_blob(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            source, out = root / "test.onnx.xz", root / "out"
            out.mkdir()
            payload = bytes(range(251)) * 3
            source.write_bytes(payload)
            models.stage(source, out, chunk_bytes=128)
            manifest = json.loads((out / "test.onnx.xz.json").read_text())
            self.assertEqual(manifest["bytes"], len(payload))
            self.assertTrue(all(c["bytes"] <= 128 for c in manifest["chunks"]))
            self.assertEqual(b"".join((out / c["file"]).read_bytes() for c in manifest["chunks"]), payload)
            self.assertFalse((out / source.name).exists())
            first = manifest["chunks"][0]["file"]
            source.write_bytes(b"changed" + payload[7:])
            models.stage(source, out, chunk_bytes=128)
            changed = json.loads((out / "test.onnx.xz.json").read_text())
            self.assertNotEqual(changed["chunks"][0]["file"], first)
            self.assertEqual(changed["chunks"][1:], manifest["chunks"][1:])

    def test_verified_install_reused_without_download_or_mutation(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            original = root / "model.onnx"
            original.write_bytes(b"model")
            spec = self.spec()
            def unexpected(*args, **kwargs):
                self.fail("verified local model should not be downloaded")
            self.assertEqual(models.detector(spec, root, root / "cache", unexpected), original)
            self.assertEqual(original.read_bytes(), b"model")
            self.assertFalse((root / "cache").exists())

    def test_corruption_rejected_and_partial_download_removed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            original = root / "model.onnx"
            original.write_bytes(b"wrong")
            with self.assertRaisesRegex(ValueError, "checksum"):
                models.detector(self.spec(), root, root / "cache", lambda *a, **k: io.BytesIO(b"wrong"))
            self.assertEqual(list((root / "cache").iterdir()), [])
            self.assertEqual(original.read_bytes(), b"wrong")
            result = models.detector(self.spec(), root, root / "cache", lambda *a, **k: io.BytesIO(b"model"))
            self.assertEqual(result.read_bytes(), b"model")

    def test_production_pin_is_read_from_rust_catalogue(self):
        spec = models.foreground_spec((models.ROOT / "crates/neural/src/lib.rs").read_text())
        self.assertEqual(spec["file"], "foreground-birefnet-lite.onnx")
        self.assertGreater(spec["bytes"], 200_000_000)
        self.assertTrue(spec["url"].startswith("https://github.com/ZhengPeng7/BiRefNet/"))
        self.assertEqual(len(spec["sha256"]), 64)

    @staticmethod
    def spec():
        return {"file": "model.onnx", "url": "https://example.test/model", "bytes": 5,
                "sha256": hashlib.sha256(b"model").hexdigest()}


if __name__ == "__main__":
    unittest.main()
