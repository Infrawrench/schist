#!/usr/bin/env python3
"""Generate the OpenEXR interoperability fixtures with the reference library.

Needs the `OpenEXR` (3.x) and `numpy` Python packages, which wrap the
Academy Software Foundation's C++ OpenEXR library:

    python3 -m venv /tmp/exr && /tmp/exr/bin/pip install OpenEXR numpy
    /tmp/exr/bin/python fixtures/exr/generate.py

Every image follows `expected()` below so the Rust tests can check
values without a reference decoder. Colour channels are stored
premultiplied by alpha, as OpenEXR requires.
"""

import os

import numpy as np
import OpenEXR

HERE = os.path.dirname(os.path.abspath(__file__))
W, H = 40, 24


def expected():
    """Straight-alpha RGBA, float32, shape (H, W, 4)."""
    y, x = np.mgrid[0:H, 0:W].astype(np.float32)
    rgba = np.empty((H, W, 4), np.float32)
    rgba[..., 0] = x / (W - 1) * 4.0  # an HDR ramp, 0..4
    rgba[..., 1] = y / (H - 1)
    rgba[..., 2] = 0.25
    rgba[..., 3] = np.where(x < W // 2, 1.0, 0.5)
    return rgba


def premultiplied(rgba):
    out = rgba.copy()
    out[..., :3] *= rgba[..., 3:4]
    return out


def write(name, header, channels):
    path = os.path.join(HERE, name)
    with OpenEXR.File(header, channels) as f:
        f.write(path)
    print(name, os.path.getsize(path))


def rgba_channels(dtype, prefix=""):
    p = premultiplied(expected()).astype(dtype)
    return {f"{prefix}{c}": np.ascontiguousarray(p[..., i]) for i, c in enumerate("RGBA")}


def main():
    compressions = {
        "none": OpenEXR.NO_COMPRESSION,
        "rle": OpenEXR.RLE_COMPRESSION,
        "zips": OpenEXR.ZIPS_COMPRESSION,
        "zip": OpenEXR.ZIP_COMPRESSION,
        "piz": OpenEXR.PIZ_COMPRESSION,
        "pxr24": OpenEXR.PXR24_COMPRESSION,
        "b44": OpenEXR.B44_COMPRESSION,
        "b44a": OpenEXR.B44A_COMPRESSION,
        "dwaa": OpenEXR.DWAA_COMPRESSION,
        "dwab": OpenEXR.DWAB_COMPRESSION,
    }
    for name, compression in compressions.items():
        header = {"compression": compression, "type": OpenEXR.scanlineimage}
        write(f"half-{name}.exr", header, rgba_channels(np.float16))

    # Float samples, tiled 16x16, so partial tiles at the right and bottom.
    tiles = OpenEXR.TileDescription()
    tiles.xSize = 16
    tiles.ySize = 16
    tiles.mode = OpenEXR.ONE_LEVEL
    header = {
        "compression": OpenEXR.ZIP_COMPRESSION,
        "type": OpenEXR.tiledimage,
        "tiles": tiles,
    }
    write("float-tiled.exr", header, rgba_channels(np.float32))

    # Data window smaller than and offset inside a display window with a
    # non-zero origin, plus ACEScg chromaticities.
    full = premultiplied(expected())
    dx, dy, dw, dh = 3, 2, 30, 20
    crop = full[dy : dy + dh, dx : dx + dw]
    header = {
        "compression": OpenEXR.PIZ_COMPRESSION,
        "type": OpenEXR.scanlineimage,
        "displayWindow": (np.array([100, 50], np.int32), np.array([100 + W - 1, 50 + H - 1], np.int32)),
        "dataWindow": (
            np.array([100 + dx, 50 + dy], np.int32),
            np.array([100 + dx + dw - 1, 50 + dy + dh - 1], np.int32),
        ),
        "chromaticities": (0.713, 0.293, 0.165, 0.830, 0.128, 0.044, 0.32168, 0.33767),
    }
    channels = {c: np.ascontiguousarray(crop[..., i].astype(np.float16)) for i, c in enumerate("RGBA")}
    write("windows-acescg.exr", header, channels)

    # Luminance with alpha.
    e = expected()
    header = {"compression": OpenEXR.ZIP_COMPRESSION, "type": OpenEXR.scanlineimage}
    write(
        "luminance-alpha.exr",
        header,
        {
            "Y": np.ascontiguousarray((e[..., 1] * e[..., 3]).astype(np.float16)),
            "A": np.ascontiguousarray(e[..., 3].astype(np.float16)),
        },
    )

    # A render: beauty plus passes in one part, including float depth and
    # a uint object id.
    channels = rgba_channels(np.float16)
    channels.update(
        {
            "diffuse.R": np.full((H, W), 0.5, np.float16),
            "diffuse.G": np.full((H, W), 0.25, np.float16),
            "diffuse.B": np.full((H, W), 0.125, np.float16),
            "depth.Z": np.full((H, W), 123.5, np.float32),
            "id.object": np.full((H, W), 7, np.uint32),
        }
    )
    write("passes.exr", {"compression": OpenEXR.ZIP_COMPRESSION, "type": OpenEXR.scanlineimage}, channels)

    # Multi-part: an unnamed-looking beauty part and a DWAB-compressed pass.
    beauty = OpenEXR.Part(
        {"compression": OpenEXR.PIZ_COMPRESSION, "type": OpenEXR.scanlineimage},
        rgba_channels(np.float16),
        "beauty",
    )
    spec = OpenEXR.Part(
        {"compression": OpenEXR.DWAB_COMPRESSION, "type": OpenEXR.scanlineimage},
        {c: np.full((H, W), 2.0, np.float16) for c in "RGB"},
        "specular",
    )
    path = os.path.join(HERE, "multipart.exr")
    with OpenEXR.File([beauty, spec]) as f:
        f.write(path)
    print("multipart.exr", os.path.getsize(path))


if __name__ == "__main__":
    main()
