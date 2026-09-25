#!/usr/bin/env python3
"""Train an experimental lens-scatter restoration model for Schist.

Paired folders: TRAIN/{input,target}, VAL/{input,target}, matching relative names.
Synthetic folders: TRAIN/{clean,flares}, VAL/{clean,flares}; optional light_sources
contains matching flare filenames with only the light source, as in Flare7K++.
See docs/anti-smudge.md for dataset sources, attribution and evaluation limits.
"""
import argparse
import hashlib
import json
import math
import random
from functools import lru_cache
from pathlib import Path

import numpy as np
from PIL import Image, ImageOps
import torch
from torch import nn
from torch.nn import functional as F

DILATIONS = (1, 2, 4, 8, 16, 32, 16, 8, 4, 2, 1)
RESIDUAL_DILATIONS = (1, 2, 4, 8, 16, 8)
CONTEXT = sum(DILATIONS) + 1  # 95 pixels; Schist trims 96.
TILE = 384
OVERLAP = 96
EXTENSIONS = {".png", ".jpg", ".jpeg", ".webp", ".tif", ".tiff"}


class ResidualBlock(nn.Module):
    def __init__(self, channels, dilation):
        super().__init__()
        self.layers = nn.Sequential(
            nn.Conv2d(channels, channels, 3, padding=dilation, dilation=dilation),
            nn.LeakyReLU(.1),
            nn.Conv2d(channels, channels, 3, padding=dilation, dilation=dilation))

    def forward(self, x):
        return x + .1 * self.layers(x)


class AntiSmudgeNet(nn.Module):
    """Small restoration CNN with short skips and no batch norm.

    Starts at identity. Dilated convolutions keep the export in tract's simple
    operator subset. The residual version has a 161-pixel receptive field;
    the legacy dilated stack has 191. This is a trainable baseline, not equivalence
    to the larger published flare-removal networks.
    """
    def __init__(self, channels=24, architecture="residual"):
        super().__init__()
        self.architecture = architecture
        if architecture == "pyramid":
            def conv(a, b):
                return nn.Sequential(nn.Conv2d(a, b, 3, padding=1), nn.LeakyReLU(.1))
            self.enc0 = conv(3, channels)
            self.enc1 = nn.Sequential(conv(channels, channels * 2), conv(channels * 2, channels * 2))
            self.middle = nn.Sequential(conv(channels * 2, channels * 4),
                                        *[ResidualBlock(channels * 4, d) for d in (1, 2, 4)])
            self.dec1 = conv(channels * 6, channels * 2)
            self.dec0 = conv(channels * 3, channels)
            self.tail = nn.Conv2d(channels, 3, 3, padding=1)
            nn.init.zeros_(self.tail.weight)
            nn.init.zeros_(self.tail.bias)
            return
        layers = []
        previous = 3
        for dilation in DILATIONS:
            conv = nn.Conv2d(previous, channels, 3, padding=dilation, dilation=dilation)
            nn.init.kaiming_normal_(conv.weight, nonlinearity="relu")
            nn.init.zeros_(conv.bias)
            layers.extend([conv, nn.ReLU()])
            previous = channels
        if architecture == "residual":
            # Short skips prevent the feature signal collapsing across the
            # long dilated stack. Radius = 1 + 2*sum(dilations) + 1 = 80.
            layers = [nn.Conv2d(3, channels, 3, padding=1), nn.LeakyReLU(.1)]
            layers += [ResidualBlock(channels, d) for d in RESIDUAL_DILATIONS]
        elif architecture != "dilated":
            raise ValueError(f"unknown architecture: {architecture}")
        self.body = nn.Sequential(*layers)
        self.tail = nn.Conv2d(channels, 3, 3, padding=1)
        nn.init.zeros_(self.tail.weight)
        nn.init.zeros_(self.tail.bias)

    def forward(self, x):
        # Do not clamp during training: gradients must reach over/underexposure.
        if self.architecture == "pyramid":
            a = self.enc0(x)
            b = self.enc1(F.avg_pool2d(a, 2))
            c = self.middle(F.avg_pool2d(b, 2))
            b = self.dec1(torch.cat((b, F.interpolate(c, size=b.shape[-2:], mode="nearest")), dim=1))
            a = self.dec0(torch.cat((a, F.interpolate(b, size=a.shape[-2:], mode="nearest")), dim=1))
            return x + self.tail(a)
        return x + self.tail(self.body(x))


def files(folder):
    paths = sorted(p for p in folder.rglob("*") if p.suffix.lower() in EXTENSIONS)
    if not paths:
        raise ValueError(f"no images in {folder}")
    return paths


@lru_cache(maxsize=128)
def rgb(path):
    with Image.open(path) as image:
        return np.asarray(ImageOps.exif_transpose(image).convert("RGB"), dtype=np.float32) / 255


def linear(image):
    return np.where(image <= .04045, image / 12.92, ((image + .055) / 1.055) ** 2.4)


def srgb(image):
    image = image.clip(0, 1)
    return np.where(image <= .0031308, image * 12.92, 1.055 * image ** (1 / 2.4) - .055)


def compose(scene, flare, light=None, gain=1.0):
    """Add scattering in linear light, retaining the true source in the target.

    Without a light annotation, retain a small core around the brightest point.
    This approximation is for pretraining; real aligned pairs are preferred for
    validation. Noise, movement and defocus are not labelled as smudge.
    """
    flare_linear = linear(flare)
    if light is None:
        luminance = flare.max(axis=2)
        y, x = np.unravel_index(luminance.argmax(), luminance.shape)
        yy, xx = np.ogrid[:flare.shape[0], :flare.shape[1]]
        radius = max(1.0, min(flare.shape[:2]) * .006)
        mask = np.exp(-((xx - x) ** 2 + (yy - y) ** 2) / (2 * radius ** 2))
        # No invented light core for a flare whose source is outside the frame.
        mask *= float(luminance[y, x] >= .8)
        source = flare_linear * mask[..., None]
    else:
        source = linear(light)
    clean = linear(scene)
    return srgb(clean + gain * flare_linear).astype(np.float32), srgb(clean + gain * source).astype(np.float32)


def resize(image, width, height):
    pil = Image.fromarray(np.round(image.clip(0, 1) * 255).astype(np.uint8))
    return np.asarray(pil.resize((width, height), Image.Resampling.BILINEAR), dtype=np.float32) / 255


class Corpus:
    def __init__(self, root, mode):
        self.root, self.mode = Path(root), mode
        if mode == "paired":
            self.scenes = files(self.root / "target")
            self.inputs = [self.root / "input" / p.relative_to(self.root / "target") for p in self.scenes]
            if any(not p.is_file() for p in self.inputs):
                raise ValueError("input and target must have matching relative filenames")
            if set(files(self.root / "input")) != set(self.inputs):
                raise ValueError("unmatched input images")
            self.flares = []
        else:
            self.scenes, self.flares = files(self.root / "clean"), files(self.root / "flares")
            self.inputs = []
            if (self.root / "light_sources").exists():
                for p in self.flares:
                    if not (self.root / "light_sources" / p.relative_to(self.root / "flares")).is_file():
                        raise ValueError("missing light-source annotation")

    def sample(self, rng, patch, identity_probability=0.0, focus_probability=0.0):
        i = rng.randrange(len(self.scenes))
        target = rgb(self.scenes[i])
        if self.mode == "paired":
            image = rgb(self.inputs[i])
            if image.shape != target.shape:
                raise ValueError(f"unaligned dimensions for {self.scenes[i]}")
        else:
            # Compose before cropping: the light responsible for a streak may
            # lie outside the crop. Both flare and source get the same transform.
            height, width = target.shape[:2]
            p = rng.choice(self.flares)
            flare = rgb(p)
            light_path = self.root / "light_sources" / p.relative_to(self.root / "flares")
            light = rgb(light_path) if light_path.is_file() else None
            rotation, flip = rng.randrange(4), rng.random() < .5
            def transform(a):
                a = np.rot90(a, rotation)
                if flip:
                    a = a[:, ::-1]
                return resize(a, width, height)
            flare = transform(flare)
            light = transform(light) if light is not None else None
            image, target = compose(target, flare, light, gain=rng.uniform(.25, 1.5))
        height, width = target.shape[:2]
        if min(height, width) < patch:
            raise ValueError(f"image smaller than patch={patch}: {self.scenes[i]}")
        y, x = rng.randrange(height - patch + 1), rng.randrange(width - patch + 1)
        if focus_probability > 0 and rng.random() < focus_probability:
            # Include affected regions more often, without conditioning validation
            # on this sampling policy. Clean controls are still sampled below.
            candidates = [(y, x)] + [(rng.randrange(height - patch + 1),
                                      rng.randrange(width - patch + 1)) for _ in range(3)]
            y, x = max(candidates, key=lambda p: float(np.abs(
                image[p[0]:p[0]+patch, p[1]:p[1]+patch] -
                target[p[0]:p[0]+patch, p[1]:p[1]+patch]).mean()))
        image, target = image[y:y+patch, x:x+patch], target[y:y+patch, x:x+patch]
        if rng.random() < identity_probability:
            image = target.copy()
        if rng.random() < .5:
            image, target = image[:, ::-1], target[:, ::-1]
        if rng.random() < .5:
            image, target = image[::-1], target[::-1]
        tensor = lambda a: torch.from_numpy(a.copy()).permute(2, 0, 1)
        return tensor(image), tensor(target)


def hashes(paths):
    return {hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}


def check_split(train, val):
    """Reject duplicate files across splits, not just duplicate pathnames.

    Perceptual duplicates and adjacent frames still require a scene-level audit.
    """
    for name in ("scenes", "flares", "inputs"):
        if hashes(getattr(train, name)) & hashes(getattr(val, name)):
            raise ValueError(f"training/validation leakage: duplicate {name}")


def loss_fn(prediction, target, degraded=None):
    difference = (prediction - target).abs()
    if degraded is None:
        pixel = difference.mean()
    else:
        # Give scatter correction a gradient even when unaffected pixels dominate.
        weights = 1 + 3 * ((degraded - target).abs().mean(1, keepdim=True) / .1).clamp(0, 1)
        pixel = (difference * weights).mean() / weights.mean()
    dx = F.l1_loss(prediction[..., 1:] - prediction[..., :-1], target[..., 1:] - target[..., :-1])
    dy = F.l1_loss(prediction[..., 1:, :] - prediction[..., :-1, :], target[..., 1:, :] - target[..., :-1, :])
    return pixel + .1 * (dx + dy)


@torch.no_grad()
def evaluate(model, corpus, patch, count, device, seed):
    rng = random.Random(seed)
    model.eval()
    errors = np.zeros(7, dtype=np.float64)
    for _ in range(count):
        bad, good = corpus.sample(rng, patch)
        bad, good = bad[None].to(device), good[None].to(device)
        restored, control = model(bad).clamp(0, 1), model(good).clamp(0, 1)
        errors += [F.mse_loss(bad, good).item(), F.mse_loss(restored, good).item(),
                   F.l1_loss(restored, good).item(), F.l1_loss(control, good).item(),
                   F.l1_loss(bad, good).item(), 0, 0]
        affected = ((bad - good).abs().mean(1, keepdim=True) > .05).expand_as(good)
        if affected.any():
            errors[5] += (restored - good).abs()[affected].mean().item()
            errors[6] += (bad - good).abs()[affected].mean().item()
    errors /= count
    psnr = lambda mse: -10 * math.log10(max(mse, 1e-12))
    return dict(input_psnr=psnr(errors[0]), restored_psnr=psnr(errors[1]),
                restored_mae=errors[2], clean_mae=errors[3], input_mae=errors[4],
                affected_restored_mae=errors[5], affected_input_mae=errors[6])


def export(model, path):
    import onnx
    model = model.cpu().eval()
    path.parent.mkdir(parents=True, exist_ok=True)
    torch.onnx.export(model, (torch.zeros(1, 3, TILE, TILE),), str(path),
                      input_names=["input"], output_names=["output"],
                      dynamic_axes={"input": {2: "h", 3: "w"}, "output": {2: "h", 3: "w"}},
                      opset_version=11, dynamo=False)
    graph = onnx.load(str(path))
    onnx.checker.check_model(graph)
    onnx.helper.set_model_props(graph, {"schist.model": "anti-smudge", "schist.status": "experimental",
                                       "schist.tile": str(TILE), "schist.overlap": str(OVERLAP),
                                       "schist.color": "sRGB unit RGB NCHW"})
    onnx.save(graph, str(path))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--train", type=Path, required=True)
    parser.add_argument("--val", type=Path, required=True)
    parser.add_argument("--mode", choices=["paired", "synthetic"], required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--steps", type=int, default=20000)
    parser.add_argument("--batch", type=int, default=4)
    parser.add_argument("--patch", type=int, default=256)
    parser.add_argument("--channels", type=int, default=24)
    parser.add_argument("--architecture", choices=["residual", "dilated", "pyramid"], default="residual")
    parser.add_argument("--lr", type=float, default=2e-4)
    parser.add_argument("--device", default="cuda" if torch.cuda.is_available() else "cpu")
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--eval-every", type=int, default=500)
    parser.add_argument("--eval-samples", type=int, default=32)
    parser.add_argument("--initialize", type=Path, help="fine-tune a checkpoint from a previous run")
    args = parser.parse_args()
    if args.out.suffix.lower() != ".onnx":
        parser.error("--out must end in .onnx (checkpoints use the .pt suffix)")
    if min(args.steps, args.batch, args.channels, args.threads, args.eval_every, args.eval_samples) < 1:
        parser.error("counts must be positive")
    if args.patch < 2 or args.lr <= 0 or not math.isfinite(args.lr):
        parser.error("patch must be >= 2 and learning rate finite and positive")
    if args.architecture == "pyramid" and (args.patch < 4 or args.patch % 4):
        parser.error("pyramid patches must be multiples of four")
    torch.set_num_threads(args.threads)
    torch.manual_seed(args.seed)
    rng = random.Random(args.seed)
    train, val = Corpus(args.train, args.mode), Corpus(args.val, args.mode)
    check_split(train, val)
    model = AntiSmudgeNet(args.channels, args.architecture).to(args.device)
    if args.device == "cpu":
        model = model.to(memory_format=torch.channels_last)
    if args.initialize:
        state = torch.load(args.initialize, map_location="cpu", weights_only=True)
        if state.get("architecture", "dilated") != args.architecture:
            parser.error("initial checkpoint architecture differs from --architecture")
        model.load_state_dict(state["weights"])
    optimizer = torch.optim.Adam(model.parameters(), lr=args.lr)
    scheduler = torch.optim.lr_scheduler.CosineAnnealingLR(optimizer, args.steps)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    metrics = evaluate(model, val, args.patch, args.eval_samples, args.device, args.seed + 10000)
    best = metrics["restored_mae"] + metrics["clean_mae"]
    history = [dict(step=0, **metrics)]
    print(json.dumps(history[-1]), flush=True)
    torch.save(dict(weights=model.state_dict(), channels=args.channels, architecture=args.architecture,
                    step=0), args.out.with_suffix(".pt"))
    for step in range(1, args.steps + 1):
        model.train()
        samples = [train.sample(rng, args.patch, identity_probability=.25,
                                focus_probability=.75) for _ in range(args.batch)]
        bad, good = [torch.stack(items).to(args.device) for items in zip(*samples)]
        if args.device == "cpu":
            bad = bad.contiguous(memory_format=torch.channels_last)
        loss = loss_fn(model(bad), good, bad)
        if not torch.isfinite(loss):
            raise RuntimeError("non-finite training loss")
        optimizer.zero_grad(set_to_none=True)
        loss.backward()
        nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        optimizer.step()
        scheduler.step()
        if step == 1 or step % 50 == 0:
            print(f"step {step}/{args.steps} loss={loss.item():.6f}", flush=True)
        if step % args.eval_every == 0 or step == args.steps:
            metrics = evaluate(model, val, args.patch, args.eval_samples, args.device, args.seed + 10000)
            history.append(dict(step=step, **metrics))
            args.out.with_suffix(".history.json").write_text(json.dumps(history, indent=2) + "\n")
            print(json.dumps(history[-1]), flush=True)
            # Keep the latest candidate for diagnosis even if clean-image damage
            # prevents it becoming the best checkpoint used for final export.
            latest = dict(weights=model.state_dict(), channels=args.channels,
                          architecture=args.architecture, step=step)
            temporary = args.out.with_suffix(".latest.pt.tmp")
            torch.save(latest, temporary)
            temporary.replace(args.out.with_suffix(".latest.pt"))
            score = metrics["restored_mae"] + metrics["clean_mae"]
            if score < best:
                best = score
                checkpoint = dict(weights=model.state_dict(), channels=args.channels,
                                  architecture=args.architecture, step=step)
                temp = args.out.with_suffix(".pt.tmp")
                torch.save(checkpoint, temp)
                temp.replace(args.out.with_suffix(".pt"))
    state = torch.load(args.out.with_suffix(".pt"), map_location="cpu", weights_only=True)
    model.load_state_dict(state["weights"])
    export(model, args.out)
    report = dict(status="experimental; real-camera quality unverified", mode=args.mode,
                  seed=args.seed, best_step=state["step"], validation=history,
                  channels=args.channels, architecture=args.architecture,
                  patch=args.patch, steps=args.steps, batch=args.batch,
                  learning_rate=args.lr, torch_version=str(torch.__version__),
                  initialization_sha256=(hashlib.sha256(args.initialize.read_bytes()).hexdigest()
                                         if args.initialize else None),
                  train=str(args.train.resolve()), val=str(args.val.resolve()),
                  training_scene_sha256=sorted(hashes(train.scenes)),
                  training_input_sha256=sorted(hashes(train.inputs)),
                  validation_input_sha256=sorted(hashes(val.inputs)),
                  validation_scene_sha256=sorted(hashes(val.scenes)),
                  training_flare_sha256=sorted(hashes(train.flares)),
                  validation_flare_sha256=sorted(hashes(val.flares)),
                  model_sha256=hashlib.sha256(args.out.read_bytes()).hexdigest())
    args.out.with_suffix(".json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Exported {args.out}. Validation is not evidence of performance on your camera.")


if __name__ == "__main__":
    main()
