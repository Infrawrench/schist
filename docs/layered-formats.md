# Paint.NET and GIMP files

Schist opens and saves `.pdn` and `.xcf` through the built-in codec
registry, including file dialogs, drag and drop, Save As, Export,
gallery thumbnails, and Quick Look. Save As keeps either extension when
the document was opened in that format. The codecs are pure Rust and
also build for the browser; neither desktop editor is needed at runtime.

| Format | Import | Export |
| --- | --- | --- |
| Paint.NET `.pdn` | PDN3 with typed NRBF metadata and gzip or uncompressed BGRA chunks; legacy blend-operation objects and newer blend-mode enums | PDN3 with gzip chunks and the backwards-compatible Paint.NET 3.5 object schema; 8-bit RGB bitmap layers |
| GIMP `.xcf` | XCF versions 0–23 with raw, RLE, or zlib tiles; RGB, grayscale, and indexed pixels; 32/64-bit file offsets | XCF v012, zlib tiles, RGB/RGBA at 8-bit, 16-bit integer, or 32-bit float precision |

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

This writes `schist.pdn`, `schist.xcf`, and `schist-16.xcf`. Open the PDN
in Paint.NET and the XCF files in GIMP, checking the layer list and
properties. The first two have a 67×65 patterned background and a hidden
Multiply layer named `青 • top`, at opacity 128/255, with partially
transparent pixels near the lower-right corner. The 16-bit XCF has the
fixture's group and disabled mask.

## References

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
