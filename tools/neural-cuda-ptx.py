#!/usr/bin/env python3
"""Maintainer-only PTX export, requiring LLVM but no CUDA toolkit or GPU."""
import argparse
import hashlib
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
source = root / "crates/neural/src/cuda/kernels.cu"
destination = source.with_suffix(".ptx")
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--clang", default="clang++")
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
stamp = "// Source SHA256: " + hashlib.sha256(source.read_bytes()).hexdigest()
if args.check:
    if not destination.read_text().startswith(stamp + "\n"):
        raise SystemExit("CUDA kernel source changed: run make neural-cuda-ptx")
else:
    with tempfile.TemporaryDirectory(prefix="schist-ptx-") as temporary:
        output = Path(temporary) / "kernels.ptx"
        subprocess.run([
            args.clang, "-x", "cuda", "--cuda-device-only", "--cuda-gpu-arch=sm_75",
            "-nocudainc", "-nocudalib", "-O3", "-ffp-contract=off", "-S",
            str(source), "-o", str(output),
        ], check=True)
        ptx = output.read_text()
        if ".extern .func" in ptx:
            raise SystemExit("PTX must not depend on external device functions")
        for entry in ("tensor", "matrix", "convolution", "softmax"):
            if ".entry " + entry + "(" not in ptx:
                raise SystemExit("missing CUDA kernel: " + entry)
        destination.write_text(stamp + "\n" + ptx)
        print(f"Wrote {destination.relative_to(root)} ({destination.stat().st_size:,} bytes)")
