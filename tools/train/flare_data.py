#!/usr/bin/env python3
"""Fetch Google Research's public CC BY 4.0 flare-only dataset.

Downloads stay in the requested directory. Nothing is added to git. Records
source, attribution, upstream MD5 and local SHA-256 for every image. Other
candidate datasets and their official download links are in docs/anti-smudge.md.
"""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import random
import urllib.parse
import urllib.request

from PIL import Image

PROJECT = "https://github.com/google-research/google-research/tree/master/flare_removal"
LICENSE = "https://creativecommons.org/licenses/by/4.0/"
AUTHORS = "Yicheng Wu, Qiurui He, Tianfan Xue, Rahul Garg, Jiawen Chen, Ashok Veeraraghavan, Jonathan T. Barron"


def index():
    rows, token = [], ""
    while True:
        query = urllib.parse.urlencode(dict(prefix="lens-flare/", maxResults=1000, pageToken=token))
        url = "https://storage.googleapis.com/storage/v1/b/gresearch/o?" + query
        with urllib.request.urlopen(url, timeout=60) as response:
            page = json.load(response)
        rows.extend(item for item in page.get("items", []) if item["name"].endswith(".png"))
        token = page.get("nextPageToken")
        if not token:
            return rows


def fetch(row, out):
    # The prefix is fixed; still reject unusual paths from remote metadata.
    relative = Path(row["name"]).relative_to("lens-flare")
    if ".." in relative.parts or relative.is_absolute():
        raise ValueError("unsafe object path")
    path = out / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    size = int(row["size"])
    if size > 64 * 1024 * 1024:
        raise ValueError(f"unexpectedly large object: {relative}")
    url = "https://storage.googleapis.com/gresearch/" + urllib.parse.quote(row["name"], safe="/")
    def valid(data):
        return len(data) == size and base64.b64encode(hashlib.md5(data).digest()).decode() == row["md5Hash"]
    data = path.read_bytes() if path.exists() else b""
    if not valid(data):
        with urllib.request.urlopen(url, timeout=120) as response:
            data = response.read(size + 1)
        if not valid(data):
            raise ValueError(f"size/checksum mismatch: {relative}")
        temporary = path.with_suffix(".part")
        temporary.write_bytes(data)
        try:
            with Image.open(temporary) as image:
                image.verify()
            temporary.replace(path)
        finally:
            temporary.unlink(missing_ok=True)
    return dict(file=relative.as_posix(), source=url, license=LICENSE, attribution=AUTHORS,
                project=PROJECT, bytes=size, md5=row["md5Hash"],
                sha256=hashlib.sha256(data).hexdigest(), generation=row["generation"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--limit", type=int, default=0, help="0 downloads all; positive values make a balanced sample")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--seed", type=int, default=7)
    args = parser.parse_args()
    if args.limit < 0 or args.jobs < 1:
        parser.error("limit must be >= 0; jobs must be > 0")
    rows = index()
    if args.limit:
        # Interleave captured and simulated so a small sample includes both.
        groups = [[r for r in rows if f"/{kind}/" in r["name"]] for kind in ["captured", "simulated"]]
        rng = random.Random(args.seed)
        for group in groups:
            rng.shuffle(group)
        mixed = []
        for i in range(max(map(len, groups))):
            mixed.extend(group[i] for group in groups if i < len(group))
        rows = mixed[:args.limit]
    args.out.mkdir(parents=True, exist_ok=True)
    records = []
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for record in pool.map(lambda row: fetch(row, args.out), rows):
            records.append(record)
            if len(records) % 50 == 0:
                print(f"{len(records)}/{len(rows)}", flush=True)
    (args.out / "credits.json").write_text(json.dumps(records, indent=2) + "\n")
    (args.out / "README.txt").write_text(
        f"How to Train Neural Networks for Flare Removal (ICCV 2021)\n{AUTHORS}\n{PROJECT}\n"
        f"Flare-only images: CC BY 4.0 ({LICENSE}). Downloaded without modification.\n"
        "Keep captured sequences and related simulated apertures together when splitting.\n"
        "This is not a clean-scene dataset; scene photos need separate provenance.\n")
    print(f"Downloaded and verified {len(records)} images in {args.out}")


if __name__ == "__main__":
    main()
