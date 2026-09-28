#!/usr/bin/env python3
"""Train/export a local RGB + coarse-alpha refiner; see docs/background-removal.md.

This trains a new edge model. It does not train the separate, pretrained IS-Net
foreground detector. Synthetic coarse masks are deliberately reported as such.
"""

import io
from model_archive import sidecar, write_archive
import argparse
from collections import OrderedDict
import hashlib
import json
from pathlib import Path
import random
import time

import numpy as np
from PIL import Image
from scipy.ndimage import gaussian_filter, maximum_filter, minimum_filter
import torch
from torch import nn

SIZE = 128
HALO = 16  # Greater than the model's 11-pixel receptive-field radius.


class MatteNet(nn.Module):
    def __init__(self):
        super().__init__()
        layers = []
        for i, dilation in enumerate([1, 2, 4, 2, 1]):
            layers.extend([nn.Conv2d(4 if i == 0 else 24, 24, 3,
                                     padding=dilation, dilation=dilation), nn.ReLU()])
        self.body = nn.Sequential(*layers)
        self.head = nn.Conv2d(24, 1, 3, padding=1)
        nn.init.zeros_(self.head.weight)
        nn.init.zeros_(self.head.bias)

    def forward(self, x):
        coarse = x[:, 3:4]
        # A texture in an already certain region must not create transparency
        # or a new foreground island. Fade residuals continuously near 0 / 1;
        # keep the full correction range at uncertain boundaries.
        confidence = torch.clamp(coarse * 20, 0, 1) * torch.clamp((1-coarse) * 20, 0, 1)
        return torch.clamp(coarse + confidence * self.head(self.body(x)), 0.0, 1.0)


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def patches(root, split, count):
    """Cache lossless RGB/alpha crops. All crops inherit the source-image split."""
    manifest = json.loads((root / "manifest.json").read_text())
    key = sha256(root / "manifest.json")[:12]
    dest = root / f"patches-{key}-{split}-{count}.npy"
    if dest.exists():
        return np.load(dest, mmap_mode="r")
    rows = [e for e in manifest["examples"] if e["split"] == split]
    rng = np.random.default_rng({"train": 51, "validation": 52, "test": 53}[split])
    cache = OrderedDict()
    crops = []
    for i, row in enumerate(rows):
        if row["image"] not in cache:
            cache[row["image"]] = Image.open(root / row["image"]).convert("RGB")
            if len(cache) > 3:
                cache.popitem(last=False)
        rgb = cache[row["image"]]
        alpha = Image.open(root / row["alpha"]).convert("L")
        if rgb.size != alpha.size:
            raise ValueError(f"misaligned matte: {row}")
        # Two scales expose broad fur/hair boundaries as well as thin details.
        for scale in [1.0, 0.5]:
            size = (max(SIZE, round(rgb.width * scale)), max(SIZE, round(rgb.height * scale)))
            image = np.asarray(rgb.resize(size, Image.Resampling.BILINEAR))
            mask = np.asarray(alpha.resize(size, Image.Resampling.BILINEAR))
            band = maximum_filter(mask, size=5) > minimum_filter(mask, size=5)
            ys, xs = np.nonzero(band)
            if not len(xs):
                continue
            for _ in range(count // 2):
                j = rng.integers(len(xs))
                x = int(np.clip(xs[j] - SIZE // 2, 0, size[0] - SIZE))
                y = int(np.clip(ys[j] - SIZE // 2, 0, size[1] - SIZE))
                crops.append(np.dstack([image[y:y+SIZE, x:x+SIZE], mask[y:y+SIZE, x:x+SIZE]]))
        if (i + 1) % 100 == 0:
            print(f"{split}: cropped {i+1}/{len(rows)} mattes", flush=True)
    if not crops:
        raise ValueError(f"no nonempty mattes in {split}")
    np.save(dest, np.stack(crops))
    return np.load(dest, mmap_mode="r")


def batch(data, indices, rng, augment, detector_errors=False):
    items = np.asarray(data[indices], dtype=np.float32) / 255.0
    inputs, truth = [], []
    for item in items:
        if augment:
            item = np.rot90(item, int(rng.integers(4)))
            if rng.random() < 0.5:
                item = item[:, ::-1]
        rgb, alpha = item[..., :3].copy(), item[..., 3].copy()
        # RGB is already composited: do not use it as pure foreground color to
        # fabricate new backgrounds, which would double-composite its edges.
        if augment:
            rgb = np.clip(rgb * rng.uniform(0.8, 1.2, (1, 1, 3)), 0, 1)
        coarse = alpha
        shift = int(rng.integers(-3, 4))
        if shift:
            coarse = (maximum_filter if shift > 0 else minimum_filter)(coarse, size=2*abs(shift)+1)
        coarse = gaussian_filter(coarse, sigma=float(rng.uniform(0.7, 3.5)))
        side = int(rng.integers(12, 45))
        coarse = np.asarray(Image.fromarray(coarse).resize((side, side), Image.Resampling.BILINEAR)
                            .resize((SIZE, SIZE), Image.Resampling.BILINEAR)).copy()
        if detector_errors and rng.random() < 0.65:
            # Real detectors also give textured, uncertain interiors, unlike
            # a blurred reference matte. Vary confidence spatially without
            # changing the independently annotated target or its RGB edges.
            noise = rng.random((8, 8), dtype=np.float32)
            noise = np.asarray(Image.fromarray(noise).resize((SIZE, SIZE), Image.Resampling.BILINEAR))
            foreground_loss = float(rng.uniform(0, 0.45)) * noise
            background_leak = float(rng.uniform(0, 0.12)) * (1-noise)
            coarse = coarse * (1-foreground_loss) + (1-coarse) * background_leak
        # Some detector predictions are confidently right; retain those too.
        if augment and rng.random() < 0.1:
            coarse = alpha.copy()
        inputs.append(np.dstack([rgb, coarse]).transpose(2, 0, 1).astype(np.float32))
        truth.append(alpha[None])
    return torch.from_numpy(np.stack(inputs)), torch.from_numpy(np.stack(truth))


def interior(tensor):
    return tensor[..., HALO:-HALO, HALO:-HALO]


@torch.no_grad()
def evaluate(model, data, device, detector_errors=False):
    rng = np.random.default_rng(99)
    totals = {"coarse_mae": 0.0, "refined_mae": 0.0,
              "coarse_gradient_mae": 0.0, "refined_gradient_mae": 0.0}
    model.eval()
    for start in range(0, len(data), 16):
        x, y = batch(data, np.arange(start, min(start+16, len(data))), rng, False, detector_errors)
        prediction = model(x.to(device)).cpu()
        y = interior(y)
        for name, p in [("coarse", x[:, 3:4]), ("refined", prediction)]:
            p = interior(p)
            totals[name+"_mae"] += (p-y).abs().mean().item() * len(x)
            g = ((torch.diff(p, dim=-1)-torch.diff(y, dim=-1)).abs().mean()
                 + (torch.diff(p, dim=-2)-torch.diff(y, dim=-2)).abs().mean()) / 2
            totals[name+"_gradient_mae"] += g.item() * len(x)
    return {key: value/len(data) for key, value in totals.items()}


@torch.no_grad()
def contact_sheet(model, data, device, dest):
    x, y = batch(data, np.arange(min(12, len(data))), np.random.default_rng(99), False)
    p = model(x.to(device)).cpu()
    canvas = Image.new("RGB", (SIZE * 4, SIZE * len(x)), "white")
    checker = (np.indices((SIZE, SIZE)).sum(0) // 12 % 2 * 0.3 + 0.5)[..., None]
    for row in range(len(x)):
        rgb = x[row, :3].permute(1, 2, 0).numpy()
        for col, a in enumerate([None, y[row, 0].numpy(), x[row, 3].numpy(), p[row, 0].numpy()]):
            image = rgb if a is None else rgb*a[..., None]+checker*(1-a[..., None])
            canvas.paste(Image.fromarray((image.clip(0, 1)*255).astype("uint8")), (col*SIZE, row*SIZE))
    canvas.save(dest)


@torch.no_grad()
def reference_output(model, dest):
    """Deterministic, non-photographic fixture for the Rust/torch parity test."""
    y, x = np.indices((SIZE, SIZE), dtype=np.float32)
    inputs = np.stack([x/127, y/127, ((x+y) % 17)/16, ((x-y+8)/16).clip(0, 1)])[None]
    model(torch.from_numpy(inputs)).numpy().astype("<f4").tofile(dest)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--steps", type=int, default=2500)
    parser.add_argument("--batch", type=int, default=16)
    parser.add_argument("--device", choices=["cpu", "mps", "cuda", "auto"], default="auto")
    parser.add_argument("--resume", type=Path)
    parser.add_argument("--detector-errors", action="store_true", help="augment uncertain interiors and faint background leakage")
    args = parser.parse_args()
    if args.steps < 1 or args.batch < 1:
        parser.error("steps and batch must be positive")
    torch.set_num_threads(4)
    torch.manual_seed(7)
    random.seed(7)
    rng = np.random.default_rng(7)
    device = args.device
    if device == "auto":
        device = "cuda" if torch.cuda.is_available() else "mps" if torch.backends.mps.is_available() else "cpu"
    args.out.parent.mkdir(parents=True, exist_ok=True)
    train = patches(args.data, "train", 6)
    validation = patches(args.data, "validation", 2)
    model = MatteNet().to(device)
    if args.resume:
        model.load_state_dict(torch.load(args.resume, map_location=device, weights_only=True)["model"])
    optimizer = torch.optim.Adam(model.parameters(), lr=0.001)
    schedule = torch.optim.lr_scheduler.CosineAnnealingLR(optimizer, args.steps, eta_min=0.00005)
    best, history, start_time = float("inf"), [], time.time()
    checkpoint = sidecar(args.out, ".pt")
    print(f"training on {device}: {len(train)} train / {len(validation)} validation crops", flush=True)
    for step in range(1, args.steps+1):
        model.train()
        x, y = batch(train, rng.integers(len(train), size=args.batch), rng, True, args.detector_errors)
        x, y = x.to(device), y.to(device)
        prediction = interior(model(x))
        truth = interior(y)
        weight = 1+4*(interior(x[:, 3:4])-truth).abs()
        loss = ((prediction-truth).abs()*weight).mean()
        loss = loss + 0.5*((torch.diff(prediction, dim=-1)-torch.diff(truth, dim=-1)).abs().mean()
                           +(torch.diff(prediction, dim=-2)-torch.diff(truth, dim=-2)).abs().mean())
        optimizer.zero_grad(set_to_none=True)
        loss.backward()
        optimizer.step()
        schedule.step()
        if step % 50 == 0:
            print(f"step {step}/{args.steps} loss={loss.item():.6f} elapsed={time.time()-start_time:.1f}s", flush=True)
        if step % 250 == 0 or step == args.steps:
            metrics = evaluate(model, validation, device, args.detector_errors)
            history.append({"step": step, **metrics})
            print(json.dumps(history[-1]), flush=True)
            if metrics["refined_mae"] < best:
                best = metrics["refined_mae"]
                torch.save({"model": model.state_dict(), "step": step}, checkpoint)
    saved = torch.load(checkpoint, map_location="cpu", weights_only=True)
    model.cpu().load_state_dict(saved["model"])
    model.eval()
    test = patches(args.data, "test", 2)
    metrics = evaluate(model, test, "cpu")
    stress_metrics = evaluate(model, test, "cpu", True)
    sample, _ = batch(test, np.arange(1), np.random.default_rng(99), False)
    buffer = io.BytesIO()
    torch.onnx.export(model, sample, buffer, input_names=["rgb_alpha"],
                      output_names=["alpha"], opset_version=17, dynamo=False)
    import onnx
    import onnxruntime as ort
    raw = buffer.getvalue()
    onnx.checker.check_model(onnx.load_model_from_string(raw))
    session = ort.InferenceSession(raw, providers=["CPUExecutionProvider"])
    with torch.no_grad():
        expected = model(sample).numpy()
    actual = session.run(None, {"rgb_alpha": sample.numpy()})[0]
    parity = float(np.max(np.abs(expected-actual)))
    if parity > 1e-5:
        raise ValueError(f"ONNX export differs by {parity}")
    reference_output(model, args.out.with_name("matting-reference.f32"))
    manifest = json.loads((args.data / "manifest.json").read_text())
    report = {
        "model": "Schist local alpha refiner (not a foreground detector)",
        "seed": 7, "steps": args.steps, "selected_step": saved["step"],
        "revision": 2, "detector_error_augmentation": args.detector_errors,
        "warm_start_sha256": sha256(args.resume) if args.resume else None,
        "device": device, "torch": torch.__version__, "batch": args.batch,
        "training_seconds": time.time()-start_time, "parameters": sum(p.numel() for p in model.parameters()),
        "dataset": {k: v for k, v in manifest.items() if k not in ["files", "examples"]},
        "dataset_manifest_sha256": sha256(args.data / "manifest.json"),
        "groups": {s: sorted({e["group"] for e in manifest["examples"] if e["split"] == s})
                   for s in ["train", "validation", "test"]},
        "patches": {"train": len(train), "validation": len(validation), "test": len(test)},
        "validation": history, "test_synthetic_coarse_masks": metrics,
        "test_synthetic_confidence_errors": stress_metrics,
        "onnx_max_abs_error": parity, **write_archive(args.out, raw),
        "limitations": ["Metrics measure refinement of synthetically degraded instance mattes, not end-to-end background removal.",
                        "Cannot recover subjects missed by the foreground detector.",
                        "Hair, fur, transparency, and ambiguous multi-subject scenes still require visual review."]}
    sidecar(args.out, ".json").write_text(json.dumps(report, indent=2)+"\n")
    contact_sheet(model, test, "cpu", sidecar(args.out, ".png"))
    print(json.dumps(report, indent=2), flush=True)


if __name__ == "__main__":
    main()
