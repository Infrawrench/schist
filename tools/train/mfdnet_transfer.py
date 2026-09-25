#!/usr/bin/env python3
"""Fine-tune published MFDNet weights on real smudged/clean pairs for Schist.

The base network is the authors' pretrained MFDNet. Reference code and weights
are fetched separately, pinned by revision/hash, and never bundled with Schist.
Requires einops plus the dependencies of tools/train/anti_smudge.py.
"""
import argparse
import hashlib
import json
from pathlib import Path
import random
import sys
import urllib.request

import torch

from anti_smudge import Corpus, check_split, evaluate, hashes, loss_fn
from small_halos import HaloValidation, add_small_halos, halo_error

PROJECT = "https://github.com/Jiang-maomao/flare-removal"
REVISION = "a9431498477ba8bf8f608d7f5fde1f841ced0a56"
WEIGHTS_URL = PROJECT + "/releases/download/checkpoint/mfdnet.pth"
WEIGHTS_HASH = "133cba2947ad0534da378c0db425aec700a386a4dae41577058d8b61fbc93ad6"
SOURCES = {
    "models/__init__.py": "eac08274b2d9a819dfde2d2840bbf08bc80e6d802f656189a4d57da2450ac1af",
    "models/model.py": "64124d6e2af79f31e65820535a9de755ffc6cba5e34811851755d591b6e28d8e",
    "models/backbone.py": "2e5f123bcc97e03e8fd1541c01bf69126d4bad1ee29e7bad1ebf922effff64cb",
    "models/blocks.py": "a124f907033390d835fb67c2f10229b627125121e5036012c0affda08ef7ef83",
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fetch(root):
    urls = [("mfdnet.pth", WEIGHTS_URL)] + [
        (name, f"https://raw.githubusercontent.com/Jiang-maomao/flare-removal/{REVISION}/{name}")
        for name in [*SOURCES, "README.md"]]
    for name, url in urls:
        destination = root / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(url, timeout=120) as response:
            data = response.read(32 * 1024 * 1024 + 1)
        if len(data) > 32 * 1024 * 1024:
            raise ValueError("unexpectedly large reference file")
        temporary = destination.with_suffix(destination.suffix + ".tmp")
        temporary.write_bytes(data)
        temporary.replace(destination)


class Restoration(torch.nn.Module):
    def __init__(self, network, light_start=.5, light_end=.65):
        super().__init__()
        self.network = network
        self.light_start = light_start
        self.light_end = light_end

    def forward(self, image):
        restored = self.network(image)
        # MFDNet's published inference pipeline restores light sources after
        # removing flare. This soft recovery also covers underexposed sources;
        # a near-white-only cutoff erased the lamp in the supplied night photo.
        t = ((image.amax(1, keepdim=True) - self.light_start) /
             (self.light_end - self.light_start)).clamp(0, 1)
        mask = t * t * (3 - 2 * t)
        return restored * (1 - mask) + image * mask


def load_reference(root, initialize=None, light_start=.5, light_end=.65):
    for name, expected in {**SOURCES, "mfdnet.pth": WEIGHTS_HASH}.items():
        if digest(root / name) != expected:
            raise ValueError(f"reference hash mismatch: {name}")
    sys.path.insert(0, str(root))
    if (root / "deps").is_dir():
        sys.path.insert(0, str(root / "deps"))
    from models import Model
    network = Model()
    state = torch.load(initialize or root / "mfdnet.pth", map_location="cpu", weights_only=True)
    state = state.get("state_dict", state)
    network.load_state_dict({k.removeprefix("module."): v for k, v in state.items()})
    return Restoration(network, light_start, light_end)


def save_weights(model, path):
    temporary = path.with_suffix(".tmp")
    torch.save(model.network.state_dict(), temporary)
    temporary.replace(path)


def train_model(model, args):
    train, val = Corpus(args.train, "paired"), Corpus(args.val, "paired")
    check_split(train, val)
    rng = random.Random(args.seed)
    def validate():
        metrics = evaluate(model, val, args.patch, args.eval_samples, args.device, 10011)
        if args.small_halo_probability:
            synthetic = evaluate(model, HaloValidation(val), args.patch, args.eval_samples,
                                 args.device, 20011)
            metrics.update({f"small_halo_{k}": v for k, v in synthetic.items()})
        return metrics

    def score(metrics):
        result = metrics["restored_mae"] + metrics["clean_mae"]
        if args.small_halo_probability:
            result += metrics["small_halo_restored_mae"] + metrics["small_halo_clean_mae"]
            result += .25 * metrics["small_halo_affected_restored_mae"]
        return result

    metrics = validate()
    history = [dict(step=0, **metrics)]
    print(json.dumps(history[-1]), flush=True)
    best, best_step = score(metrics), 0
    checkpoint = args.out.with_suffix(".pt")
    save_weights(model, checkpoint)
    optimizer = torch.optim.Adam(model.parameters(), lr=args.lr)
    for step in range(1, args.steps + 1):
        model.train()
        samples = [train.sample(rng, args.patch, focus_probability=args.focus_probability)
                   for _ in range(args.batch)]
        if args.small_halo_probability:
            samples = [add_small_halos(good, rng) if rng.random() < args.small_halo_probability
                       else (bad, good) for bad, good in samples]
        bad, good = [torch.stack(values).to(args.device) for values in zip(*samples)]
        gain = rng.uniform(.55, 1.)
        bad, good = bad * gain, good * gain
        restored, control = model(torch.cat([bad, good])).chunk(2)
        loss = loss_fn(restored, good) + loss_fn(control, good)
        if args.halo_loss:
            loss = loss + args.halo_loss * halo_error(restored, good, bad)
        if not torch.isfinite(loss):
            raise RuntimeError("non-finite training loss")
        optimizer.zero_grad(set_to_none=True)
        loss.backward()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.)
        optimizer.step()
        if step % 25 == 0:
            print(f"step={step} loss={loss.item():.6f}", flush=True)
        if step % args.eval_every == 0 or step == args.steps:
            metrics = validate()
            history.append(dict(step=step, **metrics))
            print(json.dumps(history[-1]), flush=True)
            if score(metrics) < best:
                best = score(metrics)
                best_step = step
                save_weights(model, checkpoint)
            args.out.with_suffix(".history.json").write_text(json.dumps(history, indent=2) + "\n")
    model.network.load_state_dict(torch.load(checkpoint, map_location="cpu", weights_only=True))
    return dict(best_step=best_step, steps=args.steps, patch=args.patch, batch=args.batch,
                learning_rate=args.lr, seed=args.seed, validation_seed=10011,
                focus_probability=args.focus_probability,
                small_halo_probability=args.small_halo_probability, halo_loss=args.halo_loss,
                small_halo_validation_seed=20011 if args.small_halo_probability else None,
                validation_crops=args.eval_samples, validation=history,
                training_exposure_gain=[.55, 1.],
                training_scene_sha256=sorted(hashes(train.scenes)),
                training_input_sha256=sorted(hashes(train.inputs)),
                validation_scene_sha256=sorted(hashes(val.scenes)),
                validation_input_sha256=sorted(hashes(val.inputs)),
                checkpoint_sha256=digest(checkpoint))


def export(model, path, tile_size=1024, working_max_side=768, halo_cleanup=False):
    import onnx

    class Bounded(torch.nn.Module):
        def __init__(self):
            super().__init__()
            self.model = model.cpu().eval()

        def forward(self, image):
            return self.model(image).clamp(0, 1)

    # One large frame covers the bounded working image, avoiding seams from
    # MFDNet's spatial attention statistics changing between neighboring tiles.
    torch.onnx.export(Bounded().eval(), (torch.zeros(1, 3, tile_size, tile_size),), str(path),
                      opset_version=11, dynamo=False, input_names=["input"], output_names=["output"])
    graph = onnx.load(str(path))
    onnx.checker.check_model(graph)
    properties = {"schist.model": "anti-smudge", "schist.status": "experimental",
        "schist.source": PROJECT, "schist.tile": str(tile_size), "schist.overlap": "96",
        "schist.restore_max_side": str(working_max_side), "schist.light_source_recovery":
        f"smoothstep(max_RGB; {model.light_start}..{model.light_end})"}
    if halo_cleanup:
        properties["schist.halo_cleanup"] = "radial-v1"
    onnx.helper.set_model_props(graph, properties)
    onnx.save(graph, str(path))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--fetch", action="store_true")
    parser.add_argument("--initialize", type=Path)
    parser.add_argument("--train", type=Path)
    parser.add_argument("--val", type=Path)
    parser.add_argument("--steps", type=int, default=300)
    parser.add_argument("--batch", type=int, default=2)
    parser.add_argument("--patch", type=int, default=256)
    parser.add_argument("--lr", type=float, default=1e-5)
    parser.add_argument("--seed", type=int, default=17)
    parser.add_argument("--focus-probability", type=float, default=.5,
                        help="probability of favoring a crop with stronger flare")
    parser.add_argument("--light-start", type=float, default=.5)
    parser.add_argument("--light-end", type=float, default=.65)
    parser.add_argument("--small-halo-probability", type=float, default=0,
                        help="replace this fraction of real pairs with controlled small halos")
    parser.add_argument("--halo-loss", type=float, default=0,
                        help="extra loss on positive scatter outside bright target cores")
    parser.add_argument("--eval-every", type=int, default=50)
    parser.add_argument("--eval-samples", type=int, default=16)
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--tile-size", type=int, default=1024)
    parser.add_argument("--working-max-side", type=int, default=768)
    parser.add_argument("--halo-cleanup", action="store_true",
                        help="request Schist's conservative radial glow cleanup after inference")
    parser.add_argument("--device", default="cuda" if torch.cuda.is_available() else "cpu")
    args = parser.parse_args()
    if args.out.suffix != ".onnx" or bool(args.train) != bool(args.val):
        parser.error("--out must be .onnx; supply --train and --val together")
    if min(args.steps, args.batch, args.eval_every, args.eval_samples, args.threads) < 1:
        parser.error("counts must be positive")
    if args.patch < 32 or args.patch % 32 or not 0 < args.lr < float("inf"):
        parser.error("patch must be a multiple of 32; learning rate must be finite and positive")
    if not 0 <= args.focus_probability <= 1:
        parser.error("focus probability must be between zero and one")
    if not 0 <= args.small_halo_probability <= 1 or not 0 <= args.halo_loss < float("inf"):
        parser.error("small halo probability must be in 0..1 and halo loss finite and nonnegative")
    if not 0 <= args.light_start < args.light_end <= 1:
        parser.error("light recovery thresholds must satisfy 0 <= start < end <= 1")
    if not 256 <= args.tile_size <= 2048 or args.tile_size % 32:
        parser.error("tile size must be a multiple of 32 in 256..2048")
    if not 32 <= args.working_max_side <= args.tile_size - 192:
        parser.error("working image must fit inside the tile with 96 pixels of context per side")
    if args.initialize and args.initialize.resolve() == args.out.with_suffix(".pt").resolve():
        parser.error("use a new output name to preserve the initialization checkpoint")
    torch.set_num_threads(args.threads)
    torch.manual_seed(args.seed)
    if args.fetch:
        fetch(args.reference)
    model = load_reference(args.reference, args.initialize, args.light_start, args.light_end).to(args.device)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    report = dict(status="experimental MFDNet transfer", project=PROJECT, revision=REVISION,
                  source_sha256=SOURCES, base_checkpoint_sha256=WEIGHTS_HASH,
                  initialization_sha256=digest(args.initialize) if args.initialize else WEIGHTS_HASH,
                  tile_size=args.tile_size, working_max_side=args.working_max_side,
                  halo_cleanup="radial-v1" if args.halo_cleanup else None,
                  light_source_recovery=dict(start=args.light_start, end=args.light_end,
                                             function="smoothstep of max RGB"))
    if args.train:
        report.update(train_model(model, args))
    export(model, args.out, args.tile_size, args.working_max_side, args.halo_cleanup)
    report["model_sha256"] = digest(args.out)
    args.out.with_suffix(".json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"Exported {args.out}", flush=True)


if __name__ == "__main__":
    main()
