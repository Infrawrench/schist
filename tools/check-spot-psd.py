#!/usr/bin/env python3
"""Independent wire validation using psd-tools (optional developer dependency).

Install psd-tools into your Python environment, then run make verify-spot-psd.
No Photoshop or Adobe SDK is needed. This checks standard fields, not Schist's
private visibility/backup blocks and not press colorimetry.
"""
import struct
import sys
from pathlib import Path
from psd_tools import PSDImage
from psd_tools.constants import Resource

paths = sorted(Path(sys.argv[1]).glob("spot-*.*"))
assert len(paths) == 6, f"Expected six probe files, found {len(paths)}"
for path in paths:
    psd = PSDImage.open(path)
    assert psd.size == (3, 1)
    names = psd.image_resources.get_data(Resource.ALPHA_NAMES_UNICODE)
    assert list(names) == ["Cyan — 特別"], (path, names)
    ids = psd.image_resources.get_data(Resource.ALPHA_IDENTIFIERS)
    assert list(ids) == [42], (path, ids)
    info = psd.image_resources.get_data(Resource.DISPLAY_INFO)
    assert info.version == 1 and len(info.alpha_channels) == 1
    channel = info.alpha_channels[0]
    assert int(channel.mode) == 2
    assert (channel.color_space, channel.c1, channel.c2, channel.c3, channel.opacity) == (0, 0, 65535, 65535, 35)
    alternate = psd.image_resources.get_data(Resource.ALTERNATE_SPOT_COLORS)
    assert alternate == struct.pack(">HHI5H", 1, 1, 42, 0, 0, 65535, 65535, 0)
    planes = psd._record.image_data.get_data(psd._record.header)
    assert len(planes) == 5
    depth = psd.depth
    code, scale = {8: ("B", 255), 16: ("H", 65535), 32: ("f", 1)}[depth]
    samples = [v / scale for v in struct.unpack(">" + code * 3, planes[-1])]
    assert all(abs(a - b) <= 0.002 for a, b in zip(samples, [1.0, 0.25, 0.0])), (path, samples)
    process = [v / scale for v in struct.unpack(">" + code * 3, planes[0])]
    assert all(abs(v - 0.25) <= 0.002 for v in process), (path, process)
    print(f"{path.name}: standard spot metadata, plate polarity and process pixels verified")
