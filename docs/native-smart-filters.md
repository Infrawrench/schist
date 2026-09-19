# Native PSD smart-filter interchange

Schist writes editable PSD/PSB smart filters for stacks containing Gaussian Blur,
Box Blur, Motion Blur, Median, High Pass, and Unsharp Mask. Adjustable Sharpen
exports as Unsharp Mask with radius 1 and threshold 0, retaining its amount control.
A native `SoLd` placement describes the ordered filters
and their enabled states; its `lnk2` entry embeds the original pixels as a separate
PSD. Schist also retains its lossless `ScFs`/`ScFo` blocks and the visible raster
cache. Other readers can therefore display the cache and readers supporting these
smart-filter descriptors can edit the native recipe.

The initial bridge supports RGB sources with at most 16 million pixels, an invertible affine
placement, and a filter region containing the source's complete bounds.
Motion Blur angles and Unsharp Mask thresholds must be integral; parameter ranges follow Schist's controls.
Unsupported stacks keep their private recipe and rendered pixels. Export never
silently omits an unsupported effect from a native recipe. The two applications'
filter kernels may produce different pixels when an imported recipe is changed.

Import recognizes these native filter classes on unwarped, unscaled embedded RGB PSD/PSB
sources at integer positions. Source and destination bit depths and embedded ICC profiles must match. It
requires full-opacity normal filter blending and no enabled filter mask. External
links, other embedded formats, filter masks, transformed imported sources and unknown effects
remain opaque native data with a raster fallback. Native import does not recursively
promote smart objects inside embedded files.

The private `ScFi` marker tracks placements managed by Schist. Editing a supported
stack regenerates its native recipe; changing its source creates a new content-based
linked-file ID. Embedded ICC profiles and resolution are included in source identity.
Native placement corners retain smart-object and version-2 raster-stack affine
transforms without scaling the stored source or filter parameters. Native-only
reopening of adjustable Sharpen produces an editable Unsharp Mask recipe.
Baking a stack removes its managed placement. Unrecognized original
native blocks remain untouched. Existing source records are preserved, including
unreferenced records after baking, and newly generated source records share one
`lnk2` block with them.

## Evidence and reproduction

The binary layout was implemented from the public MIT-licensed
[ag-psd additional-information implementation](https://github.com/Agamnentzar/ag-psd/blob/master/src/additionalInfo.ts),
particularly `SoLd`, `createLnkHandler`, and the filter-FX serializers. No Adobe
headers or plugin-host code were consulted.

`crates/codec-psd/tests/fixtures/native-smart-filters-ag-psd.psd` is independently
written with ag-psd 31.0.0; the adjacent `generate-native-smart-filters.cjs` reproduces
it. The test checks imported order, enabled state, parameters, translated source
pixels and the distinct display cache. Add Noise remains unsupported because the native
random seed has no matching Schist parameter. PSD and PSB tests also check native-only
reopening, edited settings, affine placement, source/profile replacement, baking and
unsupported-original preservation.

To exercise the independent reader after installing ag-psd in a temporary directory:

```sh
SCHIST_NATIVE_PSD_EXPORT_DIR=/tmp/schist-native-exports make check-psd-interchange
NODE_PATH=/tmp/schist-psd-independent/node_modules node \
  crates/codec-psd/tests/fixtures/validate-native-smart-filters.cjs \
  /tmp/schist-native-exports
```

This is structural interoperability verified with ag-psd. It has not been validated
in the proprietary Photoshop application, and is not a claim of complete PSD
smart-object or smart-filter compatibility.
