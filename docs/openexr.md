# OpenEXR

Schist opens and exports `.exr` through the built-in codec registry:
file dialogs, drag and drop, File ▸ Export, the gallery (thumbnails and
archives), the MCP server and the library API. (macOS previews EXR in
Finder itself, so Schist's Quick Look extensions do not claim it.) The
codec is the pure-Rust [`exr`](https://crates.io/crates/exr) crate
(BSD-3-Clause), so it needs nothing at runtime and builds for desktop,
mobile and the browser. The desktop app already linked `exr` through
gpui's `image` dependency, so the codec adds little to the binary; it is
built without rayon so the browser build stays single-threaded.

## Import

| | Supported |
| --- | --- |
| Storage | Scanline and tiled; for mipmapped and ripmapped files the full-resolution level |
| Samples | half, float and uint (uint is converted to float as-is, e.g. object IDs) |
| Compression | none, RLE, ZIPS, ZIP, PIZ, PXR24, B44, B44A, DWAA, DWAB |
| Channels | `R`/`G`/`B`/`A`, `Y` (+`A`), and data channels such as `N.X`/`N.Y`/`N.Z` or `depth.Z` |
| Layers | Multi-part files and channel-prefix layers (`diffuse.R`, …) |
| Windows | Display window → canvas; data window → layer position, keeping pixels outside the canvas |
| Colour | `chromaticities` → a linear-light ICC profile (linear Rec. 709 when absent) |

An EXR becomes a 32-bit document. Its samples are kept exactly:
EXR stores scene-linear light, so the document is tagged with a
matrix/shaper profile that has the file's primaries and white point and
identity tone curves, and the display transform does the encoding.
Values above 1.0 are kept; the canvas display clips them, as for other
32-bit documents. Colour is premultiplied in EXR and straight in Schist,
so it is divided by alpha on the way in. NaN becomes 0 and infinities
the largest half-float value, so they cannot poison compositing.

Each EXR layer becomes a Schist layer named after it (`part.prefix` when
both exist). The unprefixed beauty pass is the visible base layer; the
other passes are stacked above it hidden, because render passes are not
meant to be composited over each other. A layer without colour channels
maps its first three data channels to RGB in file order, and a single
channel to grey. Files Schist exported with layers rebuild the original
stack instead (see below).

Not supported: deep data, and luminance/chroma files with subsampled
`RY`/`BY` channels; both are refused with an error. HTJ2K-compressed
files cannot be read by the crate yet. Extra channels in a layer that
already has RGB (such as per-channel alpha `AR`/`AG`/`AB`) are ignored.
Header sizes are checked before anything is decoded; decoded samples
plus the document tiles they become are limited to 6 GiB (1 GiB on
32-bit targets such as the browser).

## Export

File ▸ Export offers, for OpenEXR:

- **Samples:** half float (16-bit) or float (32-bit).
- **Compression:** none, RLE, ZIPS, ZIP (default), PIZ, PXR24, B44, B44A.
  PXR24 is lossy for float samples; B44/B44A are lossy for half samples
  and leave float samples uncompressed. DWAA/DWAB are read but not
  written: the crate has no DWA encoder.
- **Layers:** also write each visible pixel layer.
- **Alpha:** write the `A` channel (colour is premultiplied either way,
  so leaving alpha out composites over black).

Every export is one scanline part. The flattened image is the
unprefixed `R`, `G`, `B` (and `A`), so any reader shows the full
picture. With layers on, each visible pixel layer (any layer except
groups and adjustment layers, inside visible groups) is added under its
own name as `name.R`, `name.G`, … over the full canvas, rendered as it
would look on its own: mask, effects and opacity applied, blend mode
and clipping not. Duplicate names get a numeric suffix. A
`schistLayers` text-vector attribute records the order, which is how
Schist rebuilds the stack on import; other readers ignore it.

The output is scene-linear with a `chromaticities` attribute:

- A document already in a linear RGB profile (an imported EXR) is
  written unchanged with its own primaries.
- An sRGB document — including 8- and 16-bit documents and 32-bit HDR
  merges, which store sRGB-encoded values — is decoded with the
  extended sRGB curve, so values above 1.0 survive, and written with
  Rec. 709 primaries. Display P3 and other matrix/shaper profiles keep
  their primaries.
- Other profiles (LUT-based) are converted to linear Rec. 709 by the
  colour engine, which clips to the profile's range.
- CMYK, Lab and grayscale documents are exported from their RGB
  composite as sRGB.

The export dialog notes that an 8- or 16-bit document is converted to
linear float. Export recipes do not offer OpenEXR; recipes are for
delivery formats.

The MCP `export` tool and the library's `export` request take the same
settings as `compression` (`none`, `rle`, `zips`, `zip`, `piz`, `pxr24`,
`b44`, `b44a`), `layered` and `alpha`; `bit_depth: 32` selects float.

## Gallery

Thumbnails and the comparison view composite the visible layers and
tone-map them for display: 1.0 is diffuse white, brighter values roll
off through the same shoulder as HDR PNG and HEIC import rather than
clipping, then the result is converted from the file's primaries to
sRGB. Archiving an unedited EXR keeps the file as it is.

## Validation

`cargo test -p schist-codecs-common --lib openexr` covers round trips
for every sample type and compression, layered export, data and display
windows, alpha, chromaticities, and the reference-library fixtures in
[`fixtures/exr`](../fixtures/exr/README.md), which include DWAA/DWAB,
tiled, multi-part and render-pass files. `fixtures/exr/verify_exports.py`
reads Schist's exports with the reference OpenEXR library.
