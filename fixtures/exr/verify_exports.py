#!/usr/bin/env python3
"""Check Schist's OpenEXR exports with the reference library.

    SCHIST_EXR_INTEROP_DIR=/tmp/schist-exr \
        cargo test -p schist-codecs-common --lib openexr::fixture_tests::write_interop_exports
    /tmp/exr/bin/python fixtures/exr/verify_exports.py /tmp/schist-exr

The exports are `float-tiled.exr` (see generate.py) plus an opaque 8x8
"glow" layer of (8, 4, 2) at (4, 4), written layered, in every
compression, as half and as float.
"""

import glob
import os
import sys

import numpy as np
import OpenEXR

from generate import H, W, expected, premultiplied


def main(directory):
    files = sorted(glob.glob(os.path.join(directory, "schist-*.exr")))
    if not files:
        sys.exit(f"no exports in {directory}")
    failures = 0
    for path in files:
        name = os.path.basename(path)
        _, compression, bits = name[:-4].split("-")
        lossy = (compression in ("b44", "b44a") and bits == "16") or compression == "pxr24"
        enum_name = "no" if compression == "none" else compression
        with OpenEXR.File(path, separate_channels=True) as f:
            part = f.parts[0]
            header = part.header
            channels = part.channels
            want_type = np.float32 if bits == "32" else np.float16
            names = sorted(channels)
            problems = []
            if names != sorted(
                ["R", "G", "B", "A", "Background.R", "Background.G", "Background.B", "Background.A",
                 "glow.R", "glow.G", "glow.B", "glow.A"]
            ):
                problems.append(f"channels {names}")
            if any(c.pixels.dtype != want_type for c in channels.values()):
                problems.append("sample type")
            if header["compression"].name.lower() != f"{enum_name}_compression":
                problems.append(f"compression {header['compression']}")
            chroma = header.get("chromaticities")
            if chroma is None or abs(chroma[0] - 0.64) > 1e-4:
                problems.append(f"chromaticities {chroma}")
            if header.get("schistLayers") is None:
                problems.append("schistLayers attribute missing")

            # The flattened beauty: the ramp with the glow composited over it.
            beauty = premultiplied(expected())
            beauty[4:12, 4:12] = [8.0, 4.0, 2.0, 1.0]
            got = np.dstack([channels[c].pixels.astype(np.float32) for c in "RGBA"])
            tolerance = 0.1 if lossy and bits == "16" else 1e-4 if lossy else 2e-3 if bits == "16" else 1e-5
            error = np.abs(got - beauty) / np.maximum(np.abs(beauty), 1.0)
            if error.max() > tolerance:
                problems.append(f"beauty error {error.max():.5f}")
            glow = channels["glow.R"].pixels.astype(np.float32)
            if abs(glow[5, 5] - 8.0) > 0.01 or glow[0, 0] != 0.0:
                problems.append("glow layer")
            print(f"{name}: {'ok' if not problems else ', '.join(problems)}")
            failures += bool(problems)
    if failures:
        sys.exit(f"{failures} export(s) failed")


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "/tmp/schist-exr")
