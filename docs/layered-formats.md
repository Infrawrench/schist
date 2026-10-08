# Paint.NET, GIMP, OpenRaster and Krita files

Schist opens and saves `.pdn`, `.xcf` and `.ora`, and opens `.kra` and `.krz`, through the built-in codec
registry, including file dialogs, drag and drop, Save As, Export,
gallery thumbnails, and Quick Look. Save As keeps either extension when
the document was opened in that format. The codecs are pure Rust and
also build for the browser; none of the desktop editors is needed at runtime.

| Format | Import | Export |
| --- | --- | --- |
| Paint.NET `.pdn` | PDN3 with typed NRBF metadata and gzip or uncompressed BGRA chunks; legacy blend-operation objects and newer blend-mode enums | PDN3 with gzip chunks and the backwards-compatible Paint.NET 3.5 object schema; 8-bit RGB bitmap layers |
| GIMP `.xcf` | XCF versions 0–23 with raw, RLE, or zlib tiles; RGB, grayscale, and indexed pixels; 32/64-bit file offsets | XCF v012, zlib tiles, RGB/RGBA at 8-bit, 16-bit integer, or 32-bit float precision |
| OpenRaster `.ora` | Stacks and PNG layers (8 or 16-bit; other raster sources the `image` crate decodes) | OpenRaster 0.0.5 with 8-bit PNG layers for 8-bit documents and 16-bit PNG layers otherwise (32-bit float values are clamped to 0–1), plus `mergedimage.png` and a 256 px thumbnail |
| Krita `.kra` / `.krz` | Paint and group layers in RGBA 8/16-bit integer or 16/32-bit float, Krita's VERSION 2 LZF tiles, transparency masks, embedded ICC profile | Not supported |

Both retain raster layer order, names, visibility, opacity, transparency,
and supported blend modes. XCF also retains groups, signed offsets,
content locks, layer masks (including disabled masks), image resolution,
and embedded ICC profiles. Grayscale and indexed XCF pixels are expanded
to RGB. High precision XCF import accepts integer, half, float, and double
samples; double and 32-bit integer samples use Schist's 32-bit float
storage. Linear RGB samples are converted to perceptual RGB. Masks use
Schist's 8-bit mask storage.

This is raster interchange, not a complete backup of either editor's
project state. XCF text is imported from its raster pixels; paths,
channels, guides, text editing information, and other editor metadata
are not round-tripped. GIMP's color spaces and compositing modes do not
all match Schist, so blended appearance can differ. Unsupported blend
operators, floating selections, and GIMP 3 live effects produce an error.
Development-era high precision XCF versions before v012, newer XCF
versions, and whole-file `.xcf.gz`/`.xcf.bz2` compression are unsupported.

Paint.NET support covers Normal, Multiply, Additive, Color Burn, Color
Dodge, Overlay, Difference, Lighten, Darken, and Screen. Reflect, Glow,
Negation, XOR, and newer operators are unsupported. Paint.NET export
requires an 8-bit RGB document with canvas-sized raster layers and does
not preserve ICC/EXIF metadata. Off-canvas pixels, groups, and masks must
be rasterized or adjusted first. Both writers reject unsupported live
layer features, including clipping, adjustments, effects, vectors, and
smart objects, rather than silently discarding them. Fill opacity is
combined with layer opacity. Save as PSD to retain Schist-specific
editing features.

The readers bound decoded buffers, tile allocations, nesting, layer
counts, and serialized object counts. NRBF is parsed as inert data: it
does not load assemblies, instantiate .NET classes, or invoke callbacks.
Malformed offsets, dimensions, runs, chunk lengths, and duplicate chunks
return errors. The default decoded-data budget is 256 MiB and the layer
limit is 4096; overhead and tile alignment count toward the budget.

## OpenRaster

Layers keep their name, visibility, opacity, signed `x`/`y` offset and
composite op; stacks become groups. A non-root stack's own `x`/`y` is
ignored, as the specification requires. A stack with `isolation="auto"`
and the default `svg:src-over` operator imports as a pass-through group,
and Schist writes pass-through groups that way; every other group is
written `isolation="isolate"`. MyPaint's and Krita's `selected` and
`edit-locked` attributes map to the active layer and the layer lock.

The baseline `svg:*` operators map to the matching Schist modes, and
Krita's `krita:<id>` extension operators are read and written for modes
the baseline lacks (Linear Burn, Vivid Light, Hard Mix, and so on).
`svg:plus` is approximated as Linear Dodge, and the destination and
atop operators as Normal. An approximated operator, and any attribute
Schist does not interpret, is kept on the layer (in a private `ScOr`
block that also survives a PSD save) and written back on the next
OpenRaster save, along with the namespace declarations it needs. Other
editors' image-level attributes and unknown elements are not kept.

Layer sources that are not rasters (MyPaint's SVG layers, say) and the
non-baseline `text` and `filter` elements are not imported. When a file
contains any, Schist adds the package's `mergedimage.png` as a visible
top layer named to say so, so the document still looks as it was saved.
The ICC profile is taken from `mergedimage.png` (or the first layer that
has one) and embedded in every PNG Schist writes. Masks must be applied
before saving, as for the other layered formats; fill opacity is
multiplied into opacity.

## Krita

Krita import reads `maindoc.xml`, the layer tree under
`<image name>/layers/`, and the document profile in
`<image name>/annotations/icc`. Paint layers keep their pixels, offset,
name, visibility, lock, opacity and composite op; group layers keep
pass-through mode and the collapsed state. A paint device's default
pixel (Krita often stores a plain background as nothing else) fills the
canvas where the device has no tile. The first transparency mask on a
paint layer becomes its layer mask; selection masks are editing aids
and are skipped.

Supported colour spaces are Krita's RGBA at 8-bit and 16-bit integer
(stored B, G, R, A) and 16-bit and 32-bit float (stored R, G, B, A).
Float layers import into a 32-bit document without clamping and keep
the document profile, which for float Krita documents is usually
linear. Krita's composite ops map to Schist modes by id, with
`soft_light_svg` approximated as Soft Light and Krita's own `hard mix`
as Hard Mix; unknown ops fall back to Normal with a log warning.

Shape (vector), fill, filter (adjustment), clone and file layers,
filter, transform and colorize masks, layers in other colour spaces,
and paint devices in the pre-2.x tile format are not imported. When any
are present Schist keeps the layers it can read and adds Krita's
`mergedimage.png` (or, in `.krz` files that have none, the small
`preview.png` scaled up) as a visible top layer whose name says layers
were left out. A whole document in CMYK, Lab, grayscale, XYZ or YCbCr
opens as that merged image alone, named with its colour space, because
the profile describes the original model rather than the RGB rendering.
Animation frames, assistants, guides, palettes, layer styles and the
proofing setup are not read. Krita export is not implemented.

Both formats are zip packages. They use the hand-written container from
`schist-codec-idml` (see [IDML](idml-format.md#why-the-container-is-hand-written)),
read with a decoded-size budget and entry limit and with each entry's
inflater capped at its declared size. PNG dimensions are checked against
the budget before decoding; XML nesting is bounded before the tree is
built; tile counts, tile payloads and LZF back references are bounds
checked.

## Validation

`make check-layered-codecs` runs codec and preview tests, including native
Paint.NET and GIMP fixtures, exports, corrupt inputs, alpha, masks, and
high precision samples. Fixture provenance is in
[`fixtures/layered/README.md`](../fixtures/layered/README.md).
`make check-layered-codecs-wasm` checks the same codecs for the browser.

To produce exports for an independent editor check:

```sh
SCHIST_LAYERED_INTEROP_DIR=/tmp/schist-interop make check-layered-codecs
```

This writes `schist.pdn`, `schist.xcf`, `schist-16.xcf`, and `schist.ora`. Open the PDN
in Paint.NET and the XCF files in GIMP, checking the layer list and
properties; open `schist.ora` in Krita, MyPaint or GIMP 3. The first three have a 67×65 patterned background and a hidden
Multiply layer named `青 • top`, at opacity 128/255, with partially
transparent pixels near the lower-right corner. The 16-bit XCF has the
fixture's group and disabled mask.

## References

The OpenRaster codec follows the
[OpenRaster specification](https://www.openraster.org/) (file layout and
layer stack). The Krita reader was written from the format behaviour of
Krita's published source (`KisTileCompressor2`, `KisLzfCompression`,
`KisKraSaver`/`KisKraLoader`) and from packages built by the tests; no
Krita code is used.
The XCF implementation follows the
[GIMP XCF specification](https://developer.gimp.org/core/standards/xcf/).
The PDN container and surface layout were checked against the public
[OpenPDN source](https://github.com/rivy/OpenPDN) and the independently
implemented [pypdn reader](https://github.com/addisonElliott/pypdn).
The NRBF record layout is documented by
[Microsoft's MS-NRBF specification](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-nrbf/75b9fe09-be15-475f-85b8-ae7b7558cfe5).

Format labels, diagnostics, and unsupported-feature descriptions use
`schist-i18n`, with translations in every registered locale. The generic
Norwegian and Serbo-Croatian catalogs are generated from Bokmål and Croatian.
