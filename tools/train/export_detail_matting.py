#!/usr/bin/env python3
"""Export the pinned ViTMatte-S detail model. No private-photo training.

The graph takes straight sRGB plus a 0 / 0.5 / 1 trimap at native resolution.
Normalization follows the authors' config, not the mirror's processor defaults.
"""
import argparse
import hashlib
import io
import json
from pathlib import Path

import numpy as np
import onnx
import torch
from huggingface_hub import snapshot_download
from transformers import VitMatteForImageMatting
from model_archive import sidecar, write_archive

REPO = "hustvl/vitmatte-small-composition-1k"
REVISION = "6a58ad7646403c1df626fbd746900aec7361ea1d"
SIDE = 768
EXPECTED_ONNX = "b6240e8404b30bd94c1e84498a03949b7d2e7e891bed85ed06ac1f8ae1d1dc58"
SOURCE_HASHES = {
    "model.safetensors": "bda9289db1bb6762d978b42d1c62ae3f34daf7497171a347a1d09657efd788cb",
    "config.json": "ae1006f5a83227048b563b2e60709d4203e432b2276949ebef41a8cfeeeaf45f",
    "README.md": "5545af4bf41cc017cd1a75ff096d27ff4e6e6ea0bb41c9a272dd4072a0fefaaf",
}


class DetailMatte(torch.nn.Module):
    def __init__(self, network):
        super().__init__()
        self.network = network
        self.register_buffer("mean", torch.tensor([.485, .456, .406]).reshape(1, 3, 1, 1))
        self.register_buffer("std", torch.tensor([.229, .224, .225]).reshape(1, 3, 1, 1))

    def forward(self, rgb_trimap):
        rgb = (rgb_trimap[:, :3] - self.mean) / self.std
        inputs = torch.cat([rgb, rgb_trimap[:, 3:4]], dim=1)
        return self.network(pixel_values=inputs).alphas


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("crates/neural/models/detail-matting.onnx.xz"))
    args = parser.parse_args()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    report_path = sidecar(args.out, ".json")
    source = Path(snapshot_download(REPO, revision=REVISION,
        allow_patterns=["config.json", "model.safetensors", "README.md"],
        local_dir=Path("target/background-removal/detail-matting-source")))
    for name, expected_hash in SOURCE_HASHES.items():
        if digest(source / name) != expected_hash:
            raise ValueError(f"pinned detail matting source differs: {name}")
    torch.set_num_threads(4)
    torch.manual_seed(19)
    model = DetailMatte(VitMatteForImageMatting.from_pretrained(
        source, local_files_only=True)).eval()
    sample = torch.rand(1, 4, SIDE, SIDE)
    sample[:, 3] = (sample[:, 3] * 3).floor().clamp(0, 2) / 2
    with torch.inference_mode():
        expected = model(sample).numpy()
    buffer = io.BytesIO()
    torch.onnx.export(model, sample, buffer, input_names=["rgb_trimap"],
        output_names=["alpha"], opset_version=17, dynamo=False)
    raw = buffer.getvalue()
    onnx.checker.check_model(onnx.load_model_from_string(raw))
    from remove_background import session
    actual = session(raw, low_memory=True).run(None, {"rgb_trimap": sample.numpy()})[0]
    error = float(np.max(np.abs(expected - actual)))
    if error > 5e-5:
        raise ValueError(f"ONNX export differs by {error}")
    if hashlib.sha256(raw).hexdigest() != EXPECTED_ONNX:
        raise ValueError("export differs from the pinned native artifact; inspect before updating its hash")
    report = {
        "model": "ViTMatte-S Composition-1K",
        "source": f"https://huggingface.co/{REPO}", "revision": REVISION,
        "source_sha256": {p.name: digest(p) for p in source.iterdir() if p.is_file()},
        "normalization_source": "https://github.com/hustvl/ViTMatte/blob/main/configs/common/model.py",
        "preprocessing": "Native-resolution straight sRGB 0..1 and trimap 0/0.5/1. ImageNet RGB normalization is embedded in the graph.",
        "input": [1, 4, SIDE, SIDE], "output": "Soft alpha; known trimap pixels are constrained by the runtime.",
        "tiling": "768 input, 128 context, 512 output, 384 stride; overlapping output windows blended with linear weights.",
        "trimap": "Foreground min(alpha, radius 4) > .98; background max(alpha, radius 4) < .02; otherwise unknown. Add foreground cores where min(Schist MatteNet alpha, radius 16) > .98, then expand those cores by radius 12, without overriding known background.",
        "opaque_core_refiner_sha256": "368329288b05675c70cc7a13fbcb0845eb1ca98620017e640fd2fc70e267073a",
        "training": "Pretrained upstream; no Schist or private-photo fine-tuning.",
        "license": "Upstream ViTMatte MIT; Hugging Face model card Apache-2.0; see models/licenses.",
        "torch": torch.__version__, "onnx_max_abs_error": error,
        **write_archive(args.out, raw),
        "limitations": ["Automatic trimaps inherit detector mistakes.",
            "Fine transparent detail is not guaranteed; ambiguous backgrounds still need review."]}
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
