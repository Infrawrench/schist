#!/usr/bin/env python3
"""Export the pinned, pretrained BiRefNet Lite Matting weights for native use.

Run `make export-foreground` with foreground-export-requirements.txt installed.
No photographs are sent anywhere. This converts upstream weights; it is not
additional training. Downloaded Python code is verified before importing it.
"""

import argparse
import gc
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import time
import types
import urllib.request

REVISION = "99c33412e3f58e1f33187abdc8c435c645243690"
REPOSITORY = "https://huggingface.co/ZhengPeng7/BiRefNet_lite-matting"
FILES = {
    "birefnet.py": "af8568b5be406bf4d2a68a7ed6d72e40f73b37a1fb6fc9ebd71b5b3cbcd069c9",
    "BiRefNet_config.py": "e7b8c2a74f6cea6a59553d517f71d47f2c1d90e670a13416af17c25fe2f3dc52",
    "model.safetensors": "ce8bcfc045e336322c0424a5863dcfb7e9ce8fed0a5fd4d1b2b20adf12d97243",
}
EXPECTED_ONNX = "273501048979b3012b544232234618819225745a13e316b21b4db439a2f28fe8"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=Path("target/background-removal/foreground-source"))
    parser.add_argument("--out", type=Path, default=Path("target/background-removal/foreground-matting.onnx"))
    parser.add_argument("--install", action="store_true")
    args = parser.parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    for name, expected in FILES.items():
        destination = args.cache / name
        if not destination.exists():
            temporary = destination.with_suffix(destination.suffix + ".download")
            urllib.request.urlretrieve(f"{REPOSITORY}/resolve/{REVISION}/{name}", temporary)
            if digest(temporary) != expected:
                raise ValueError(f"download checksum mismatch: {name}")
            temporary.replace(destination)
        if digest(destination) != expected:
            raise ValueError(f"cached source checksum mismatch: {name}")

    import numpy as np
    import onnx
    import torch
    import deform_conv2d_onnx_exporter as exporter
    from safetensors.torch import load_file
    from remove_background import session

    # Exporter 1.2.0 uses the old public location. PyTorch 2.14 moved it.
    from torch.onnx._internal.torchscript_exporter._type_utils import JitScalarType
    exporter.JitScalarType = JitScalarType
    package = types.ModuleType("local_biref")
    package.__path__ = [str(args.cache.resolve())]
    sys.modules["local_biref"] = package
    from local_biref import birefnet as architecture
    from local_biref.BiRefNet_config import BiRefNetConfig

    class Export(torch.nn.Module):
        def __init__(self, model):
            super().__init__()
            self.m = model

        def forward(self, x):
            return self.m(x)[-1]

    torch.set_num_threads(4)
    torch.manual_seed(7)
    model = architecture.BiRefNet(config=BiRefNetConfig(bb_pretrained=False)).eval()
    model.load_state_dict(load_file(str(args.cache / "model.safetensors")), strict=True)
    model = Export(model).eval()
    shapes = []
    original = architecture.deform_conv2d

    def record(*arguments, **keywords):
        result = original(*arguments, **keywords)
        shapes.append(([list(keywords[k].shape) for k in ["input", "weight", "offset", "mask"]],
                       list(result.shape)))
        return result

    # A fixed 1024-square export. Propagate the dimensions recorded during
    # tracing: current TorchScript otherwise loses spatial sizes before DCN.
    architecture.deform_conv2d = record
    original_symbolic = exporter.deform_conv2d_func(True, False)

    def symbolic(graph, image, weight, offset, mask, bias, *rest):
        inputs, output_shape = shapes.pop(0)
        for value, shape in zip([image, weight, offset, mask], inputs):
            value.setType(value.type().with_sizes(shape))
        output = original_symbolic(graph, image, weight, offset, mask, bias, *rest)
        return output.setType(image.type().with_sizes(output_shape))

    torch.onnx.register_custom_op_symbolic("torchvision::deform_conv2d", symbolic, 12)
    x = torch.rand(1, 3, 1024, 1024)
    start = time.time()
    with torch.inference_mode():
        torch.onnx.export(model, x, str(args.out), input_names=["input_image"],
                          output_names=["output_image"], opset_version=17, dynamo=False)
    if shapes:
        raise ValueError("not all recorded deformable convolutions were exported")
    architecture.deform_conv2d = original
    with torch.inference_mode():
        expected = model(x).sigmoid().numpy()
    input_array = x.numpy()
    del model, x
    gc.collect()
    onnx.checker.check_model(str(args.out))
    runtime = session(args.out, low_memory=True)
    logits = runtime.run(None, {"input_image": input_array})[0]
    error = np.abs(1/(1+np.exp(-np.clip(logits, -80, 80))) - expected)
    del runtime
    gc.collect()
    if error.max() > .002 or error.mean() > 1e-5:
        raise ValueError(f"ONNX/PyTorch probability mismatch: {error.max()}")
    report = {
        "model": "BiRefNet Lite Matting", "source": REPOSITORY, "revision": REVISION,
        "source_sha256": FILES, "onnx_sha256": digest(args.out),
        "bytes": args.out.stat().st_size, "torch": torch.__version__,
        "input": [1, 3, 1024, 1024], "output": "foreground logits; sigmoid before resizing",
        "preprocessing": "Antialiased triangle resize; sRGB float32; ImageNet mean/std",
        "training": "Pretrained upstream; no private-photo gradient training or uploads",
        "license": "BiRefNet MIT; deform_conv2d_onnx_exporter MIT",
        "max_probability_error": float(error.max()), "mean_probability_error": float(error.mean()),
        "seconds": time.time() - start,
    }
    args.out.with_suffix(".json").write_text(json.dumps(report, indent=2) + "\n")
    if report["onnx_sha256"] != EXPECTED_ONNX:
        raise ValueError("export differs from the pinned native artifact; inspect the report before updating its hash")
    if args.install:
        store = Path(os.environ.get("SCHIST_MODEL_DIR", Path(os.environ.get("XDG_DATA_HOME", Path.home()/".local/share"))/"schist/models"))
        store.mkdir(parents=True, exist_ok=True)
        destination = store / "foreground-birefnet-matting.onnx"
        temporary = destination.with_suffix(".onnx.tmp")
        shutil.copyfile(args.out, temporary)
        temporary.replace(destination)
        print(destination)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
