#!/usr/bin/env python3
"""Stage lazy-loaded neural assets; no model enters the startup manifest/WASM.

The foreground download is pinned by the Rust catalogue. Reuse a verified
local installation if present, otherwise fetch into the ignored build cache.
Payloads use <=16 MiB chunks so large models work on ordinary static hosts.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sys
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
CHUNK_BYTES = 16 * 1024 * 1024


def foreground_spec(catalogue):
    entry = re.search(r'id: "foreground",(.*?)\n    },', catalogue, re.S).group(1)
    return {
        "file": re.search(r'file: "([^"]+)"', entry).group(1),
        "url": re.search(r'ModelSource::Download\("([^"]+)"\)', entry).group(1),
        "sha256": re.search(r'sha256: Some\("([a-f0-9]{64})"\)', entry).group(1),
        "bytes": int(re.search(r'bytes: ([\d_]+)', entry).group(1).replace("_", "")),
    }


def verified(path, spec):
    if not path.is_file() or path.stat().st_size != spec["bytes"]:
        return False
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest() == spec["sha256"]


def detector(spec, installed, cache, download=urllib.request.urlopen):
    for folder in (installed, cache):
        path = folder / spec["file"]
        if verified(path, spec):
            return path
    cache.mkdir(parents=True, exist_ok=True)
    path = cache / spec["file"]
    partial = path.with_suffix(path.suffix + ".part")
    try:
        with download(spec["url"], timeout=60) as source, partial.open("wb") as dest:
            shutil.copyfileobj(source, dest, CHUNK_BYTES)
        if not verified(partial, spec):
            raise ValueError("foreground detector size/checksum mismatch")
        partial.replace(path)
    finally:
        partial.unlink(missing_ok=True)
    return path


def stage(source, out, chunk_bytes=CHUNK_BYTES):
    chunks = []
    with source.open("rb") as stream:
        while block := stream.read(chunk_bytes):
            # A deployment can replace weights without mixing cached old chunks
            # into the new model. The catalogue hash still validates the whole.
            digest = hashlib.sha256(block).hexdigest()
            name = f"{source.name}.{digest}.{len(chunks):03d}"
            (out / name).write_bytes(block)
            chunks.append({"file": name, "bytes": len(block)})
    if not chunks:
        raise ValueError(f"empty model: {source}")
    (out / f"{source.name}.json").write_text(json.dumps({
        "bytes": sum(c["bytes"] for c in chunks), "chunks": chunks,
    }) + "\n")


def check_wasm(wasm, model_dir):
    compiled = wasm.read_bytes()
    for source in [*model_dir.glob("*.onnx"), *model_dir.glob("*.onnx.xz")]:
        with source.open("rb") as stream:
            signature = stream.read(4096)
        if signature and signature in compiled:
            raise ValueError(f"model payload embedded in browser WASM: {source.name}")


def main():
    models = ROOT / "crates/neural/models"
    if sys.argv[1] == "--check-wasm":
        check_wasm(Path(sys.argv[2]), models)
        return
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    spec = foreground_spec((ROOT / "crates/neural/src/lib.rs").read_text())
    data = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))
    installed = Path(os.environ.get("SCHIST_MODEL_DIR", data / "schist/models"))
    foreground = detector(spec, installed, ROOT / "target/web-models")
    for source in [*sorted(models.glob("*.onnx")), *sorted(models.glob("*.onnx.xz")), foreground]:
        stage(source, out)
    for source in models.glob("*.json"):
        shutil.copy2(source, out / source.name)


if __name__ == "__main__":
    main()
