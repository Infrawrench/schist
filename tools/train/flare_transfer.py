#!/usr/bin/env python3
"""Export or calibrate the published Flare7K++ Uformer for Schist.

The base network is the authors' pretrained model, not a Schist-trained network.
Optional calibration learns one residual-strength parameter on real pairs and
retains it only if restoration error plus clean-control error improves on held-
out pairs. Source, checkpoint and dataset hashes are recorded. Requires einops
in addition to the ordinary Anti-Smudge training dependencies.
"""
import argparse
import hashlib
import json
from pathlib import Path
import random
import sys
import urllib.request
import zipfile

import numpy as np
import torch

from anti_smudge import Corpus, check_split, hashes
from flare_real_data import download

REVISION = "d1fb66ecb3c75fb3f4bfa715c49ef9265892d56f"
SOURCE_HASH = "7c9c793a1e7bbaaa3bc18601f3848937c247f3567c3fee9a8d69e55c0a6ec9be"
WEIGHTS_HASH = "75f0fc77ab43703c7a9c7876621f8a651d6ce3a0cfb7c6e2377b3c8e2331b0e2"
PROJECT = "https://github.com/ykdai/Flare7K"
WEIGHTS_URL = "https://drive.google.com/uc?export=download&id=17AX9BJ-GS0in9Ey7vw3BVPISm67Rpzho"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fetch(root):
    root.mkdir(parents=True, exist_ok=True)
    archive = root / "pretrained_model.zip"
    download(WEIGHTS_URL, archive, 76059895)
    with zipfile.ZipFile(archive) as zipped:
        (root / "net_g_last.pth").write_bytes(zipped.read("net_g_last.pth"))
    for name, remote in [("uformer_arch.py", "basicsr/archs/uformer_arch.py"), ("LICENSE", "LICENSE")]:
        url = f"https://raw.githubusercontent.com/ykdai/Flare7K/{REVISION}/{remote}"
        with urllib.request.urlopen(url, timeout=60) as response:
            (root / name).write_bytes(response.read())


def load_reference(root):
    if digest(root / "uformer_arch.py") != SOURCE_HASH or digest(root / "net_g_last.pth") != WEIGHTS_HASH:
        raise ValueError("reference source/checkpoint does not match the pinned published version")
    # Also supports dependencies installed in an isolated --target directory.
    if (root / "deps").is_dir():
        sys.path.insert(0, str(root / "deps"))
    source = (root / "uformer_arch.py").read_text()
    source = source.replace(
        "from timm.models.layers import DropPath, to_2tuple, trunc_normal_",
        "from torch.nn.init import trunc_normal_\n"
        "to_2tuple = lambda x: x if isinstance(x, tuple) else (x, x)\n"
        "DropPath = nn.Identity")
    source = source.replace("from basicsr.utils.registry import ARCH_REGISTRY", "")
    source = source.replace("@ARCH_REGISTRY.register()", "")
    namespace = {}
    exec(compile(source, str(root / "uformer_arch.py"), "exec"), namespace)
    # Stochastic depth is disabled for the frozen evaluation network. Registry
    # and timm's initialization helpers do not change loaded inference weights.
    model = namespace["Uformer"](img_size=512, img_ch=3, output_ch=6, drop_path_rate=0)
    state = torch.load(root / "net_g_last.pth", map_location="cpu", weights_only=True)
    model.load_state_dict(state.get("params_ema", state.get("params", state)))
    return model.eval().requires_grad_(False)


@torch.no_grad()
def predictions(model, corpus, count, seed, patch):
    rng, samples = random.Random(seed), []
    for i in range(count):
        bad, good = corpus.sample(rng, patch)
        x, g = bad[None], good[None]
        samples.append(torch.cat((x, g, model(x)[:, :3], model(g)[:, :3]), dim=1).half())
        if (i + 1) % 8 == 0:
            print(f"{corpus.root}: predictions {i + 1}/{count}", flush=True)
    return torch.cat(samples)


@torch.no_grad()
def evaluate(samples, alpha):
    totals = np.zeros(5)
    for sample in samples.float():
        x, g, p, c = sample.chunk(4, dim=0)
        y, control = (x + alpha * (p - x)).clamp(0, 1), (g + alpha * (c - g)).clamp(0, 1)
        totals += [(x - g).abs().mean().item(), (y - g).abs().mean().item(),
                   (control - g).abs().mean().item(), ((x - g)**2).mean().item(),
                   ((y - g)**2).mean().item()]
    return dict(zip(["input_mae", "restored_mae", "clean_mae", "input_mse", "restored_mse"],
                    (totals / len(samples)).tolist()))


def calibrate(model, args):
    train, val = Corpus(args.train, "paired"), Corpus(args.val, "paired")
    check_split(train, val)
    training = predictions(model, train, args.train_samples, 29, 256)
    validation = predictions(model, val, args.val_samples, 10029, 256)
    alpha = torch.tensor(1., requires_grad=True)
    optimizer = torch.optim.Adam([alpha], lr=.02)
    rng = random.Random(23)
    for step in range(args.steps):
        batch = training[[rng.randrange(len(training)) for _ in range(4)]].float()
        x, g, p, c = batch.chunk(4, dim=1)
        y = (x + alpha * (p - x)).clamp(0, 1)
        control = (g + alpha * (c - g)).clamp(0, 1)
        loss = (y - g).abs().mean() + (control - g).abs().mean()
        optimizer.zero_grad(set_to_none=True)
        loss.backward()
        optimizer.step()
        with torch.no_grad():
            alpha.clamp_(.25, 4.)
        if (step + 1) % 100 == 0:
            print(f"step={step + 1} alpha={alpha.item():.6f}", flush=True)
    alpha = float(alpha.detach())
    baseline, calibrated = evaluate(validation, 1.), evaluate(validation, alpha)
    score = lambda m: m["restored_mae"] + m["clean_mae"]
    accepted = score(calibrated) < score(baseline)
    return dict(trained_alpha=alpha, selected_alpha=alpha if accepted else 1., accepted=accepted,
                baseline=baseline, calibrated=calibrated, training_crops=args.train_samples,
                validation_crops=args.val_samples, steps=args.steps, seed=29, optimizer_seed=23,
                patch=256, prediction_cache_dtype="float16",
                training_scene_sha256=sorted(hashes(train.scenes)),
                validation_scene_sha256=sorted(hashes(val.scenes)))


def export(model, path, alpha):
    import onnx

    class Restoration(torch.nn.Module):
        def __init__(self):
            super().__init__()
            self.network = model

        def forward(self, image):
            restored = self.network(image)[:, :3]
            return (image + alpha * (restored - image)).clamp(0, 1)

    # Fixed dimensions are intentional: Uformer uses Python shape arithmetic.
    torch.onnx.export(Restoration().eval(), (torch.zeros(1, 3, 384, 384),), str(path),
                      opset_version=11, dynamo=False, input_names=["input"], output_names=["output"])
    graph = onnx.load(str(path))
    onnx.checker.check_model(graph)
    onnx.helper.set_model_props(graph, {"schist.model": "anti-smudge", "schist.status": "experimental",
        "schist.source": PROJECT, "schist.tile": "384", "schist.overlap": "96",
        "schist.restore_max_side": "768", "schist.calibration_alpha": str(alpha)})
    onnx.save(graph, str(path))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--fetch", action="store_true")
    parser.add_argument("--train", type=Path)
    parser.add_argument("--val", type=Path)
    parser.add_argument("--steps", type=int, default=600)
    parser.add_argument("--train-samples", type=int, default=48)
    parser.add_argument("--val-samples", type=int, default=16)
    parser.add_argument("--threads", type=int, default=4)
    args = parser.parse_args()
    if args.out.suffix != ".onnx" or bool(args.train) != bool(args.val):
        parser.error("--out must be .onnx; --train and --val must be supplied together")
    if min(args.steps, args.train_samples, args.val_samples, args.threads) < 1:
        parser.error("counts must be positive")
    torch.set_num_threads(args.threads)
    if args.fetch:
        fetch(args.reference)
    model = load_reference(args.reference)
    report = dict(status="experimental published Flare7K++ model", project=PROJECT,
                  revision=REVISION, source_sha256=SOURCE_HASH, base_checkpoint_sha256=WEIGHTS_HASH,
                  license="S-Lab noncommercial license; see the downloaded LICENSE",
                  selected_alpha=1., working_max_side=768)
    if args.train:
        report.update(calibrate(model, args))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    export(model, args.out, report["selected_alpha"])
    report["model_sha256"] = digest(args.out)
    args.out.with_suffix(".json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Exported {args.out}; residual calibration = {report['selected_alpha']:.6f}", flush=True)


if __name__ == "__main__":
    main()
