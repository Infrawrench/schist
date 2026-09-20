# Spot ink channels

The Color panel includes a **Spot ink** section for document-wide separations.
These are scalar ink plates, independent of RGB/CMYK/Lab layers and their alpha.
They are not colored raster layers. Each plate has a name, display color,
visibility and solidity; its normalized floating-point samples describe ink
coverage (0 = no ink, 1 = full ink).

Choose a foreground color, then **New** to create a blank plate and select the
Brush tool. The channel selector switches between process colors and named
plates. Selecting a plate shows its grayscale separation: black is full ink,
white is no ink. Brush, Pencil, Eraser, the channel **Fill** button and the
ordinary Fill/Clear commands respect the selection. **Value** sets the ink
coverage painted or filled; Eraser/Clear remove ink. Strokes are one undo step.
Copy/Cut of a selected spot use its grayscale separation; Paste into a spot
converts clipboard brightness to inverse ink coverage and respects alpha and the
selection. The existing clipboard uses 8-bit samples. Copy Merged still copies
process colors. Other paint/retouch tools are not implemented for spots. Choose the process
entry or click a layer to resume editing process layers.

Edit the name field and press Enter or **Rename**. **Foreground Color** assigns
the current foreground color to the plate's display swatch. **Hide** excludes a
plate from the overprint simulation without deleting or changing its samples.
**Delete**, metadata edits and fills are undoable. Crop, Image Size, Canvas Size,
and whole-image rotation/flip keep plates registered with the process image;
Image Size uses bilinear scalar resampling for plates, including when process
layers use a neural upscaler. Content-aware resize is unavailable while extra
plates exist because it cannot supply a shared geometric mapping for them.

## Preview

The preview selector offers process colors, the selected separation, and
**Overprint simulation**. Under **Preview**, **Opacity** adjusts the PSD solidity
parameter, independently of paint coverage. Its zero endpoint is labeled
**Transparent**: transparent ink multiplies its display color into the process
image and preceding spots. Increasing solidity mixes toward opaque ink; at
100% solidity a full-coverage spot obscures the earlier colors. New spots start
transparent. Spots are applied in channel-list order over white paper, after
the process display transform, in both native and browser canvas rendering.

This is a display approximation, not a certified press proof. It does not
model paper spectra, ink chemistry, trapping, dot gain, halftones, ink limits,
or a press-specific spot profile. Display color and solidity never modify
plate samples, process samples, or the document's ICC profile. The preview is
view-only: ordinary RGB image exports contain the process composite, not a
baked simulation. Use PSD/PSB to deliver the separate plates.

## Saving and sharing

PSD and PSB write the plates as additional merged-image channels at the
file's 8-, 16-, or 32-bit depth. On disk, spot polarity follows PSD conventions:
zero means full ink. Unicode alpha names (1045), alpha identifiers (1053) and
DisplayInfo (1077, mode 2), and Alternate Spot Colors (1067) carry native spot
names, IDs, display color and
solidity. Existing ordinary alpha channels remain in their original order and
retain their samples. Native CMYK process planes do not pass through RGB.
Files without layer records are supported too.

The reader accepts legacy Pascal names (Macintosh Roman) and DisplayInfo 1007.
RGB, CMYK, Lab and grayscale display colors are approximated on screen; unknown
custom color spaces use an available alternate color or a neutral preview while retaining their original
DisplayInfo entry. Explicitly changing a color or solidity replaces that entry
with an RGB display entry. The exact original alpha metadata is also kept in
the private `ScIr` backup; it is inactive after edits. Unknown resources and
unrelated document blocks remain preserved. Schist stores visibility separately
in `ScIn`; other applications may use their own visibility state. Preview mode
and active channel are view state and are not saved into PSD.

Shared documents and recovery snapshots store each plate's metadata and each
scalar tile as independent CRDT fields. Concurrent edits to different plates
or tiles merge independently; concurrent edits to the same tile have the
existing last-writer semantics. Recovery preserves floating-point coverage and
visibility without quantizing through a PSD metadata thumbnail. Shared input
checks limit total plane count and scalar payload bytes. PSD/PSB allow at most
56 total process, transparency and extra channels.

Affinity, Paint.NET and GIMP export reject documents with extra ink channels rather than silently
losing their separations. Other ordinary raster exports retain their existing
process-only behavior. Saved selections have not been promoted to editable
alpha channels by this feature; imported alpha planes are preserved, while
the Spot ink UI edits only actual spot plates.

## Verification and format provenance

`make test-spot-ink` checks selection-aware fill, stroke/erase undo and cancel,
COW snapshots, crop/resize registration, overlapping transparent/opaque inks,
visibility, process independence, native CMYK invariants, shared recovery and
concurrent tiles. Hand-encoded fixtures independently exercise flattened and
layered PSD/PSB at every supported depth, preserving alpha and spot planes.
`make test-spot-editor` checks rotation/flip and undo. `make check-spot-ink`
type-checks the editor; `make check-i18n` checks all shipped catalogs, aliases,
and font coverage.

With the optional Python `psd-tools` package installed, `make verify-spot-psd`
generates six small PSD/PSB files and reads them with that independent parser,
checking standard names, IDs, DisplayInfo, plate polarity and unchanged process
pixels. This validates interchange structure; Photoshop UI and physical press
output have not been manually verified.

The implementation uses the public
[Adobe file-format specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)
and the psd-tools project's published
[DisplayInfo layout](https://psd-tools.readthedocs.io/en/latest/_modules/psd_tools/psd/image_resources.html).
No Adobe header files or Photoshop plugin SDK code were read, and no Photoshop
plugin support code was modified.
