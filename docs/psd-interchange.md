# Editable PSD and PSB interchange

Schist now writes native PSD **type layers** (`TySh`, including binary
ActionDescriptors and UTF-16 EngineData) and native **smart filters**
(`SoLd.filterFX` with a matching embedded source PSD in `lnk2`). These are in
addition to the lossless Schist blocks, not replacements for those blocks.
The layer's cached pixels remain available to readers that cannot render type
or smart filters.

## Text

New horizontal left-to-right text and vertical text with columns advancing
right to left export their text and font identifiers,
font/size/bold/italic/RGBA fill runs, alignment, tracking, leading and point or
wrapped-box placement. EngineData run lengths use UTF-16 code units, while
Schist's editing ranges use UTF-8 byte offsets. Astral characters therefore do
not shift the following runs. Installed fonts are named by their PostScript
identifier. The common kerning, ligature, discretionary-ligature and small-cap
features are mapped; fonts themselves are not embedded.

Supported horizontal left-to-right and standard vertical type from another PSD
becomes editable with the Type tool. Uniform positive scale is incorporated in its size and spacing, and
translation places its baseline. The original native descriptor is retained
exactly while the imported text remains unchanged. Editing supported text
regenerates the native descriptor from the live settings, so other readers see
the new text rather than stale imported content. A stale document-wide `Txt2`
engine cache is deactivated and retained byte-for-byte in the private `ScT2`
backup; unchanged imported documents retain their original active cache. The cached raster is not
re-rendered on import. A missing local font can change layout on a subsequent
edit; the native font name remains in the unchanged original descriptor.

The present Schist type model cannot faithfully edit arbitrary affine/warped
native text, different paragraph formatting per paragraph, decorations,
overset/clipped paragraph boxes or arbitrary OpenType features. Such imported type
keeps its native descriptor and rendered pixels without being promoted to a
misleading editable local layer. Unmodified rotation/shear/warp data therefore
remains editable in a native-capable editor after Schist save. New text on paths,
vertical columns advancing left to right, directional scripts/controls requiring
bidi metadata, and unmapped OpenType overrides retain
`PsTx` and pixels only. PSD and Schist may use different line-breaking/font
metrics when an editor re-renders a layer.

Mixed RGB/alpha character fills survive text insertion, deletion, font changes,
and PSD/PSB regeneration, and render with those fills in the Type tool. Selecting
another foreground fill changes the whole layer's fill and clears character fill
overrides. Native color types other than RGB remain preserved without promotion.
Standard vertical type maps `Ornt=Vrtc`, `WritingDirection=2`, `Procession=1`,
point baselines and paragraph boxes (the wrap length is their vertical extent).
Warped/affine vertical text and custom vertical alternates remain outside the
editable subset. Neither Photoshop nor Affinity interactive rendering was tested.

## Layer effects

Imported effects are rasterized when a document is installed, before its first
canvas/thumbnail render. Native Photoshop glow descriptors keep their spread,
range and noise settings, including on save. Softer glows use a grown matte and
an approximate Gaussian falloff; existing Schist and Affinity glows retain their
intensity-based rendering. The importer also recognizes the long blend names
`normal`, `dissolve`, `darken`, `multiply` and `screen` alongside their short IDs.

`make check-psd-effects` covers import/save, visible halo coverage and CPU/GPU
agreement. The effects-only `photoshop-text-effects.lfx2` fixture was extracted
from a user-supplied PSD and contains no artwork or text. Falloff was compared
with that PSD's saved Photoshop composite; this is approximate visual matching,
not pixel-identical Photoshop rendering.

## Light adjustments

Modern Photoshop Light layers are recognized by a `brit` compatibility block
and a `CgEd` descriptor with `brightnessModeLight`. The legacy block alone can
contain all zeros even when the Light controls are active. Schist reads Exposure,
Contrast, Highlights, Shadows, Whites and Blacks from the companion descriptor,
regardless of block order, and exposes all six controls in the adjustment dialog.
Unrelated or malformed `CgEd` blocks remain preserved without changing the
adjustment's interpretation.

Untouched descriptors round-trip verbatim. Edited settings regenerate the native
Light descriptor, replacing the stale block; newly authored Light parameters
also emit the `brit` compatibility block. These are native Photoshop controls,
not private Schist metadata. No Adobe headers or SDK sources were used.

Rendering is a bounded, monotone per-channel tone-curve approximation. Exposure
has a white-preserving shoulder, contrast bends the midtones, shadows/highlights
weight the dark/bright ranges, and black/white controls set the endpoints. The
curve strengths were checked against the supplied PSD's saved composite; they
are not an independently recovered Photoshop algorithm. In particular, Adobe
documents [adaptive behavior for Highlights and Shadows](https://helpx.adobe.com/photoshop/desktop/create-manage-layers/color-adjustment-fill-layers/adjust-image-lighting-with-light.html),
which this per-pixel approximation does not reproduce. Color-profile-dependent
tone processing and exact Photoshop parity remain outside this implementation.

On the reported 1536×1920 RGB8 PSD, RGB mean absolute error against its saved
Photoshop composite decreases from 10.79 to 5.66 levels out of 255, with the glow
fix present in both renders. This single-document comparison is a visual check,
not a general fidelity guarantee. `make check-psd-light` covers native fixtures,
editing/saving, legacy fallback, curve invariants and CPU/GPU agreement. The two
352-byte `.cged` fixtures contain only adjustment settings, no artwork.

## Filters

See [native-smart-filters.md](native-smart-filters.md) for the supported Gaussian
Blur, Box Blur, Motion Blur, Median, High Pass, Unsharp Mask and adjustable
Sharpen mappings, embedded source packaging, affine export, and import limits. A stack is exported as native filters only
when every effect and its source geometry can be represented. Unsupported
stacks still retain their full `ScFs`/`ScFo` state and visible cache.

## Spot ink separations

Named spot plates and preserved alpha channels use extra merged-image planes,
Unicode alpha names, identifiers and native DisplayInfo mode 2. PSD and PSB keep
8/16/32-bit coverage independently of RGB/CMYK/Lab process channels. See
[spot-ink.md](spot-ink.md) for editing controls, display-only overprint simulation,
original metadata preservation, shared recovery, independent parser checks and
limits. This does not provide press-certified proofing.

## Independent validation and provenance

Implementation references are the public, MIT-licensed
[ag-psd type block implementation](https://github.com/Agamnentzar/ag-psd/blob/387049670cb89b88fb8fe1b7c01aeacf98dd2e3b/src/additionalInfo.ts),
[EngineData tokenizer](https://github.com/Agamnentzar/ag-psd/blob/387049670cb89b88fb8fe1b7c01aeacf98dd2e3b/src/engineData.ts), and
[text model encoder](https://github.com/Agamnentzar/ag-psd/blob/387049670cb89b88fb8fe1b7c01aeacf98dd2e3b/src/text.ts).
No Adobe header files or Photoshop plugin SDK code were read.

The checked-in text fixtures are generated by **ag-psd 31.0.2**, not by
Schist's writer. `scripts/psd-text-fixtures.cjs` records their exact generator.
Native import tests verify these fixtures, including native transform
preservation and edit/save behavior. A separate ag-psd reader verifies files
emitted by Schist (both PSD and PSB), checking text, UTF-16 run lengths, fonts,
mixed fills, paragraph alignment, vertical orientation/box dimensions and
transforms. This is independent structural
interoperability evidence; Photoshop/Photopea interactive rendering has not
been exercised here.

The text checker resolves style properties inherited from the text layer.
It accepts either an installed DejaVu Sans bold face or the writer's
synthetic-bold fallback, so installing DejaVu Sans is not a prerequisite.
Both paths still verify that the regular and bold runs remain distinct.

```sh
npm install --prefix /tmp/schist-psd-independent --ignore-scripts ag-psd@31.0.2
SCHIST_INTERCHANGE_ARTIFACT_DIR=/tmp/schist-interchange \
SCHIST_NATIVE_PSD_EXPORT_DIR=/tmp/schist-native-exports make check-psd-interchange
AG_PSD_MODULE=/tmp/schist-psd-independent/node_modules/ag-psd \
SCHIST_INTERCHANGE_ARTIFACT_DIR=/tmp/schist-interchange \
  make check-editable-interchange-independent
NODE_PATH=/tmp/schist-psd-independent/node_modules node \
  crates/codec-psd/tests/fixtures/validate-native-smart-filters.cjs /tmp/schist-native-exports
```

Parser regression tests reject malformed string escapes, excessive nesting,
inconsistent UTF-16 runs and unrepresentable geometry. Text measurement during
import allocates layout data only, never a bitmap proportional to an untrusted
font size.
