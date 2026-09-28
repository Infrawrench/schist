#!/usr/bin/env python3
"""Fetch a pinned, attributed MicroMat-3K subset, split by source photograph.

The authors link this dataset mirror from https://github.com/naver-ai/ZIM.
Its dataset card specifies CC BY 4.0 (the ZIM *code/model* has a separate
license and is not used here). These are instance mattes, not foreground
segmentation targets. Use them to train local edge refinement only.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import io
import json
from pathlib import Path
import random
import time
import urllib.request

from PIL import Image

REPO = "merve/MicroMat-3k"
REVISION = "a950e12d008de146b857227683123f2ba0ef0665"
ROOT = f"https://huggingface.co/datasets/{REPO}/resolve/{REVISION}/"


def download(path: str, root: Path) -> dict:
    dest = root / path
    dest.parent.mkdir(parents=True, exist_ok=True)
    for attempt in range(4):
        try:
            if dest.exists():
                data = dest.read_bytes()
            else:
                with urllib.request.urlopen(ROOT + path, timeout=90) as response:
                    data = response.read()
            if path.endswith(".png"):
                Image.open(io.BytesIO(data)).verify()
            dest.write_bytes(data)
            return {"path": path, "url": ROOT + path, "sha256": hashlib.sha256(data).hexdigest()}
        except Exception:
            if attempt == 3:
                raise
            time.sleep(attempt + 1)
    raise AssertionError("unreachable")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--per-image", type=int, default=6)
    parser.add_argument("--jobs", type=int, default=8)
    args = parser.parse_args()
    if args.per_image < 1 or args.jobs < 1:
        parser.error("per-image and jobs must be positive")
    args.out.mkdir(parents=True, exist_ok=True)
    with urllib.request.urlopen(f"https://huggingface.co/api/datasets/{REPO}/revision/{REVISION}") as response:
        index = json.load(response)
    files = [entry["rfilename"] for entry in index["siblings"]]
    images = sorted(p for p in files if p.startswith("MicroMat3K/img/"))
    # Instances of the same photograph MUST stay together across splits.
    rng = random.Random(20260925)
    rng.shuffle(images)
    examples, wanted = [], {"README.md"}
    for i, image in enumerate(images):
        image_id = Path(image).stem
        split = "test" if i < 30 else "validation" if i < 60 else "train"
        mattes = sorted(p for p in files if p.startswith("MicroMat3K/matte/")
                        and Path(p).stem.split("_")[0] == image_id)
        rng.shuffle(mattes)
        # Mix the two annotation granularities, without duplicating samples.
        for matte in mattes[:args.per_image]:
            examples.append({"image": image, "alpha": matte, "group": image_id, "split": split})
            wanted.update([image, matte])
    records = []
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for i, record in enumerate(pool.map(lambda p: download(p, args.out), sorted(wanted))):
            records.append(record)
            if (i + 1) % 50 == 0:
                print(f"downloaded {i + 1}/{len(wanted)}", flush=True)
    hashes = {record["path"]: record["sha256"] for record in records}
    seen = {}
    for example in examples:
        digest = hashes[example["image"]]
        if digest in seen and seen[digest] != example["split"]:
            raise ValueError("duplicate source photograph crosses dataset splits")
        seen[digest] = example["split"]
    manifest = {
        "dataset": REPO, "revision": REVISION, "license": "CC-BY-4.0",
        "attribution": "MicroMat-3K, Beomyoung Kim et al., NAVER Cloud (2024), hosted by merve",
        "source": "https://github.com/naver-ai/ZIM#dataset-preparation",
        "license_source": ROOT + "README.md",
        "purpose": "local edge refinement; this split is not the official zero-shot benchmark",
        "examples": examples, "files": records,
    }
    (args.out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({s: sum(e["split"] == s for e in examples)
                      for s in ["train", "validation", "test"]}), flush=True)


if __name__ == "__main__":
    main()
