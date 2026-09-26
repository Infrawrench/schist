#!/usr/bin/env python3
"""Install/run Schist's local TripoSR backend. Inference never accesses the network.

The reconstruction consumes the supplied alpha; it does not detect objects or
remove backgrounds. CPU marching cubes avoids a platform-specific CUDA extension.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import types
import urllib.request
import venv

SOURCE_REVISION = "107cefdc244c39106fa830359024f6a2f1c78871"
WEIGHTS_REVISION = "5b521936b01fbe1890f6f9baed0254ab6351c04a"
DINO_REVISION = "f205d5d8e640a89a2b8ef0369670dfc37cc07fc2"


def python_at(root):
    return root / "venv" / ("Scripts/python.exe" if os.name == "nt" else "bin/python")


def install(root):
    root.mkdir(parents=True, exist_ok=True)
    # A completion marker is only published after all downloads and checks.
    (root / "ready.json").unlink(missing_ok=True)
    venv.create(root / "venv", with_pip=True)
    python = str(python_at(root))
    subprocess.run([python, "-m", "pip", "install", "--upgrade", "pip"], check=True)
    torch_args = ["torch>=2.6,<3"]
    if sys.platform in ("linux", "win32") and shutil.which("nvidia-smi") is None:
        torch_args += ["--index-url", "https://download.pytorch.org/whl/cpu"]
    subprocess.run([python, "-m", "pip", "install", *torch_args], check=True)
    subprocess.run([python, "-m", "pip", "install", "numpy<2", "Pillow>=10.4",
                    "omegaconf==2.3.0", "einops==0.7.0", "transformers==4.35.0",
                    "trimesh==4.0.5", "huggingface-hub<0.18", "imageio",
                    "scikit-image"], check=True)
    with tempfile.TemporaryDirectory(dir=root) as temporary:
        archive = Path(temporary) / "source.tar.gz"
        urllib.request.urlretrieve(
            f"https://github.com/VAST-AI-Research/TripoSR/archive/{SOURCE_REVISION}.tar.gz", archive)
        with tarfile.open(archive) as source:
            # Extract only regular files inside the pinned repository directory.
            for member in source.getmembers():
                parts = Path(member.name).parts
                if len(parts) < 2 or not member.isfile() or ".." in parts:
                    continue
                destination = root / "source" / Path(*parts[1:])
                destination.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as incoming, destination.open("wb") as outgoing:
                    shutil.copyfileobj(incoming, outgoing)
    downloader = r'''
import sys
from pathlib import Path
from huggingface_hub import hf_hub_download
root, weights, dino = sys.argv[1:]
for name in ["config.yaml", "model.ckpt"]:
    hf_hub_download("stabilityai/TripoSR", name, revision=weights,
                   local_dir=str(Path(root)/"weights"), local_dir_use_symlinks=False)
hf_hub_download("facebook/dino-vitb16", "config.json", revision=dino,
               local_dir=str(Path(root)/"dino"), local_dir_use_symlinks=False)
'''
    subprocess.run([python, "-c", downloader, str(root), WEIGHTS_REVISION, DINO_REVISION], check=True)
    (root / "ready.json").write_text(json.dumps({"source": SOURCE_REVISION, "weights": WEIGHTS_REVISION}), encoding="utf-8")


def reconstruct(root, input_path, output_path, resolution):
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    import numpy as np
    import torch
    torch.set_num_threads(min(8, os.cpu_count() or 1))
    from PIL import Image
    from skimage.measure import marching_cubes
    from omegaconf import OmegaConf
    from transformers import ViTConfig

    # TripoSR expects torchmcubes' XYZ ordering, whereas skimage yields ZYX.
    # The upstream helper flips it back before mapping its density grid.
    def cpu_marching_cubes(level, threshold):
        vertices, faces, _, _ = marching_cubes(level.detach().cpu().numpy(), level=threshold, gradient_direction="ascent")
        return (torch.from_numpy(vertices[:, [2, 1, 0]].copy()),
                torch.from_numpy(faces.copy().astype(np.int64)))

    backend = types.ModuleType("torchmcubes")
    backend.marching_cubes = cpu_marching_cubes
    sys.modules["torchmcubes"] = backend
    # Upstream imports rembg in its general utilities, but reconstruction does
    # not need it. Keep that optional feature disabled, without installing its
    # separate segmentation runtime or accidentally downloading another model.
    segmentation = types.ModuleType("rembg")
    def no_segmentation(*args, **kwargs):
        raise RuntimeError("Segmentation is disabled in the reconstruction backend")
    segmentation.remove = no_segmentation
    sys.modules["rembg"] = segmentation
    sys.path.insert(0, str(root / "source"))
    from tsr.system import TSR
    from tsr.models.tokenizers.image import DINOSingleImageTokenizer

    # The upstream initializer fetches DINO's config by repo name. Bind it to
    # the exact downloaded file so an offline run cannot resolve another rev.
    def configure_tokenizer(self):
        from transformers.models.vit.modeling_vit import ViTModel
        self.model = ViTModel(ViTConfig.from_json_file(str(root / "dino" / "config.json")))
        self.register_buffer("image_mean", torch.tensor([0.485, 0.456, 0.406]).reshape(1, 1, 3, 1, 1), persistent=False)
        self.register_buffer("image_std", torch.tensor([0.229, 0.224, 0.225]).reshape(1, 1, 3, 1, 1), persistent=False)

    DINOSingleImageTokenizer.configure = configure_tokenizer
    config = OmegaConf.load(root / "weights" / "config.yaml")
    OmegaConf.resolve(config)
    model = TSR(config)
    model.load_state_dict(torch.load(root / "weights" / "model.ckpt", map_location="cpu", weights_only=True))
    device = "cuda:0" if torch.cuda.is_available() else "cpu"
    model.to(device).eval()
    model.renderer.set_chunk_size(8192)
    # Composite existing transparency onto neutral grey. Preserve every opaque
    # input pixel, including its background; no segmentation is run here.
    source = Image.open(input_path).convert("RGBA")
    source.thumbnail((1024, 1024))
    side = max(source.size)
    prepared = Image.new("RGBA", (side, side), (128, 128, 128, 255))
    prepared.alpha_composite(source, ((side-source.width)//2, (side-source.height)//2))
    with torch.inference_mode():
        codes = model([prepared.convert("RGB")], device=device)
        mesh = model.extract_mesh(codes, has_vertex_color=True, resolution=resolution)[0]
    # TripoSR uses Z-up. Export glTF's Y-up convention with the
    # reconstructed front facing +Z, ready for the editor's default camera.
    mesh.apply_transform([[0, 1, 0, 0], [0, 0, 1, 0], [1, 0, 0, 0], [0, 0, 0, 1]])
    mesh.export(output_path, file_type="glb")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("install")
    run = sub.add_parser("run")
    run.add_argument("--input", type=Path, required=True)
    run.add_argument("--output", type=Path, required=True)
    run.add_argument("--resolution", type=int, choices=[128, 192, 256], default=192)
    args = parser.parse_args()
    if args.command == "install":
        install(args.root.resolve())
    else:
        reconstruct(args.root.resolve(), args.input, args.output, args.resolution)


if __name__ == "__main__":
    main()
