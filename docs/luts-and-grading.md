# Colour lookup tables and colour grading

## Color Lookup adjustment layers

Adjust ▸ Color Lookup adds a layer that runs the image beneath it through a
lookup table. **Load LUT…** in its dialog reads:

- `.cube` files: 1D (`LUT_1D_SIZE`) or 3D (`LUT_3D_SIZE`, 2 to 129 points per
  axis), `DOMAIN_MIN`/`DOMAIN_MAX`, and the 1D-shaper-then-3D layout with
  `LUT_1D_INPUT_RANGE`/`LUT_3D_INPUT_RANGE` that Resolve writes. Red varies
  fastest, as Adobe's *Cube LUT Specification 1.0* says.
- `.3dl` files (Lustre/Flame layout): an input mesh line, then integer
  triples with blue varying fastest. The output depth comes from a
  `Mesh <in> <out>` header when present and is otherwise inferred from the
  largest value (10, 12 or 16 bits, or 0..1 floats). An uneven mesh such as
  0, 64, …, 960, 1023 is honoured through a 1D shaper.

3D lattices use tetrahedral interpolation, so neutral input stays neutral.
Out-of-domain input clamps to the table's edge, and the result clamps to
0..1. Layer opacity, blend mode, masks and clipping work as they do for any
adjustment.

The dialog's **Input** setting chooses what the table sees: *Document values*
(the encoded values, which is what Photoshop does) or *Linear light*
(sRGB-decoded before the lookup and re-encoded after), for tables made for
scene-linear data. No log encodings are offered.

The table is stored in the document. The layer keeps the file's original
bytes (zlib-compressed in its settings), so the `.cube`/`.3dl` is not needed
again. Malformed files, sizes beyond the limits above and files over 128 MB
are refused with a message; nothing is allocated from a declared size until
its entries have been read.

Rendering runs on the GPU compositor and the CPU reference alike. The GPU
receives the same table the CPU interpolates, not a resampled copy;
`compositor-gpu/tests/adjustment_coverage.rs` holds both to the same output.
Large tables are parsed once and cached rather than per tile.

Color Lookup is a layer only: it is not offered under Image ▸ Adjustments,
batch processing or the MCP catalog, because each of those would need a file
picker of its own.

### PSD and PSB

The layer maps to Photoshop's Color Lookup block, `clrL`: a version, then a
descriptor with `lookupType`, `Nm  `, `Dthr`, `LUTFormat`, `dataOrder`,
`tableOrder`, `LUT3DFileData` (the file's bytes) and `LUT3DFileName`. The field
names follow the public, MIT-licensed
[ag-psd implementation](https://github.com/Agamnentzar/ag-psd/blob/387049670cb89b88fb8fe1b7c01aeacf98dd2e3b/src/additionalInfo.ts)
and our own files; no Adobe headers or SDK sources were used.

- Embedded `.cube` and `.3dl` tables render. Abstract and device-link
  profile lookups, `.look` files and blue-first table orders parse as
  unsupported: the layer renders as a no-op and its block is kept verbatim.
- An imported block is written back byte-for-byte until its table or input
  setting changes, so fields Schist does not model survive.
- Photoshop has no input-space setting. A table used on *Linear light* is
  saved as a 33³ `.cube` with the linear conversion baked in, which renders
  the same elsewhere; Schist reopens it as that baked table in *Document
  values*.

The structure is tested against our own reader and writer only. Neither
Photoshop nor another reader has opened these files here.

## Export as a LUT

File ▸ Export ▸ Color Lookup Table… samples an identity lattice of 17³, 33³ or
65³ points through the document's adjustment layers with the active
compositor, and saves the result as a `.cube`. The test suite checks that
loading an exported table reproduces the stack it came from.

Only colour changes that are the same for every pixel can be stored:

- Included: visible adjustment layers at the top level or in pass-through
  groups, with their opacity, the groups' opacity and their blend modes.
- Left out, and counted in the status message: adjustments with an enabled
  mask, clipped adjustments, and adjustments inside isolated (non
  pass-through) groups.
- Not representable at all: pixel layers, layer effects, filters, and
  anything that looks at neighbouring pixels.

When the active layer is a camera RAW development, **Include the active
layer's Camera Raw color settings** runs its per-pixel Camera Raw controls
over the lattice first (contrast, highlights, shadows, whites, blacks,
vibrance, saturation and colour grading). Temperature, tint and exposure act
on sensor data before the picture exists, and clarity, dehaze, sharpening,
noise reduction and vignetting are spatial, so all of those are left out.

## Colour grading in Camera Raw

Camera Raw has Lightroom-style colour grading: a wheel each for shadows,
midtones and highlights and one for the whole image, with a luminance slider
per region, **Blending** and **Balance**. A wheel's angle is the tint's hue and
its distance from the centre the tint's strength.

This is Schist's own construction, not Adobe's algorithm (see
`plugins/filters-core/src/color_grading.rs`): regions are weighted by luma
with smooth boundaries at a third and two thirds, widened by Blending and
shifted by Balance; tints add a hue direction with its luma removed, so they
change colour rather than brightness; luminance lifts or lowers in proportion
to the remaining headroom. It runs after the presence controls and before
detail, on the GPU when the Camera Raw graph does, with the CPU as the
reference.

The settings are part of the RAW development and survive PSD/PSB save in a
private `ScCg` block beside `ScRw`, so files written before grading existed
read unchanged, and older Schist versions keep the block verbatim. Recorded
Camera Raw steps from before grading replay with neutral wheels.
