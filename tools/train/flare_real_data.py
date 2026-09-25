#!/usr/bin/env python3
"""Download the official FlareReal600 2K pairs and prepare a CPU training copy.

Preserves the authors' 600/50 train/validation split. Archives total about 4.4 GB.
Dataset: https://github.com/Zdafeng/FlareReal (CC BY-NC-SA 4.0).
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import html
import io
import json
from pathlib import Path
import re
import urllib.parse
import urllib.request
import zipfile

from PIL import Image

ARCHIVES = [
    ("train", "input", "1JTtQzsF6stWr1M0tuTez-zPBgkQ4BPVr", 1943793981),
    ("train", "target", "17l2VSxrdQ-PKlkZvzD6qvODgcKtlnlVr", 2130615865),
    ("val", "input", "1xTh7whQ8Cxqps91ZN6_jdiT28TmG8IIr", 157306059),
    ("val", "target", "1bsQ82CumA6gjYuFc-g1rmKe1aG6GtD2c", 172377944),
]


def download(url, path, size):
    if path.exists() and path.stat().st_size == size:
        return
    with urllib.request.urlopen(url, timeout=60) as response:
        if "text/html" in response.headers.get("Content-Type", ""):
            page = response.read().decode()
            form = re.search(r'<form[^>]+action="([^"]+)"', page)
            if not form:
                raise ValueError("Google Drive did not offer a download")
            action = html.unescape(form[1])
            if action != "https://drive.usercontent.google.com/download":
                raise ValueError("unexpected download destination")
            fields = dict(re.findall(r'<input[^>]+name="([^"]+)"[^>]+value="([^"]*)"', page))
            url = action + "?" + urllib.parse.urlencode(fields)
    temporary = path.with_suffix(".partial")
    try:
        with urllib.request.urlopen(url, timeout=120) as response, temporary.open("wb") as output:
            if "text/html" in response.headers.get("Content-Type", ""):
                raise ValueError("download returned HTML")
            count = 0
            while block := response.read(2**20):
                count += len(block)
                if count > size:
                    raise ValueError("archive exceeds published size")
                output.write(block)
        if count != size:
            raise ValueError("archive size differs from published size")
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def prepare(item, root, bounds):
    split, kind, file_id, size = item
    archive = root / f"{split}-{kind}.zip"
    url = f"https://drive.google.com/uc?export=download&id={file_id}"
    download(url, archive, size)
    print(f"Downloaded {archive}", flush=True)
    target = root / split / kind
    target.mkdir(parents=True, exist_ok=True)
    records, names = [], set()
    with zipfile.ZipFile(archive) as zipped:
        for entry in zipped.infolist():
            if (entry.is_dir() or "__MACOSX/" in entry.filename or
                    Path(entry.filename).suffix.lower() not in {".png", ".jpg", ".jpeg"}):
                continue
            name = Path(entry.filename).name
            if name in names or entry.file_size > 100 * 2**20:
                raise ValueError("duplicate image name or unexpected image size")
            names.add(name)
            data = zipped.read(entry)  # Also checks the ZIP member CRC.
            destination = target / name
            with Image.open(io.BytesIO(data)) as image:
                original_size = image.size
                image = image.convert("RGB")
                image.thumbnail(bounds, Image.Resampling.LANCZOS)
                image.save(destination)
            records.append(dict(member=entry.filename,
                                original_sha256=hashlib.sha256(data).hexdigest(),
                                original_size=original_size,
                                file=str(destination.relative_to(root)),
                                sha256=hashlib.sha256(destination.read_bytes()).hexdigest()))
    expected = 600 if split == "train" else 50
    if len(records) != expected:
        raise ValueError(f"expected {expected} images, received {len(records)}")
    with archive.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    record = dict(source=url, project="https://github.com/Zdafeng/FlareReal",
                  license="CC-BY-NC-SA-4.0", archive_sha256=digest,
                  resize_bounds=bounds, resampling="Lanczos", images=records)
    (root / f"{split}-{kind}-credits.json").write_text(json.dumps(record, indent=2) + "\n")
    print(f"Prepared {split}/{kind}: {len(records)} images", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--width", type=int, default=1024)
    parser.add_argument("--height", type=int, default=768)
    parser.add_argument("--jobs", type=int, default=4)
    args = parser.parse_args()
    if min(args.width, args.height, args.jobs) < 1:
        parser.error("dimensions and jobs must be positive")
    args.out.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        list(pool.map(lambda item: prepare(item, args.out, (args.width, args.height)), ARCHIVES))


if __name__ == "__main__":
    main()
