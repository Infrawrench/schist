# OpenEXR interoperability fixtures

Written by the reference OpenEXR library (3.5, through its `OpenEXR`
Python bindings) with `generate.py`, which also documents the pixel
values the tests expect:

```sh
python3 -m venv /tmp/exr && /tmp/exr/bin/pip install OpenEXR numpy
/tmp/exr/bin/python fixtures/exr/generate.py
```

| File | Covers |
| --- | --- |
| `half-<compression>.exr` | RGBA half, scanlines, every compression OpenEXR 3 writes except HTJ2K/ZSTD: none, RLE, ZIPS, ZIP, PIZ, PXR24, B44, B44A, DWAA, DWAB |
| `float-tiled.exr` | RGBA float in 16×16 tiles, with partial edge tiles |
| `windows-acescg.exr` | data window inside an offset display window; ACEScg chromaticities |
| `luminance-alpha.exr` | `Y` + `A` |
| `passes.exr` | beauty plus `diffuse.*`, float `depth.Z`, and uint `id.object` |
| `multipart.exr` | two parts, `beauty` (PIZ) and `specular` (DWAB) |

`verify_exports.py` checks Schist's exports with the same library; see
its docstring. All files are original test data under the repository's
MIT license.
