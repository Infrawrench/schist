# Saved export recipes

Open **File → Export → Export recipes…** to create named export recipes. In the gallery,
select photos, open **Process…** from their context menu, then choose **Export recipes…**.
This exports each selected photo's current saved edit when it has a sidecar, otherwise
its original. Unsaved canvas edits are included when exporting the open document.

The starter recipe creates a full-resolution PNG, a JPEG whose longest edge is at most
1600 pixels, and a lossless WebP thumbnail whose longest edge is at most 512 pixels.
Select an output to edit its format, longest edge, quality (JPEG only), and filename.
Add or remove outputs to keep between one and sixteen. Zero longest edge means original
size; positive values shrink while preserving the aspect ratio and never upscale.

Choose **Whole image**, **Every artboard**, or **Every slice**. Artboards and slices are
clipped to the image bounds; empty regions are skipped, and a source with no applicable
regions reports an error. A gallery recipe applies that choice separately to each photo.

Filename templates accept `{name}`, `{region}`, `{width}`, `{height}`, and `{index}`.
`name` is the document or original photo's filename without its extension; `region` is
the artboard/slice name or `canvas`; `index` is the output's one-based position. The
actual encoder extension is always appended, so a template ending in `.jpg` with PNG
selected produces `.jpg.png`. Unsupported placeholders are rejected. Directory separators,
control characters, reserved names, and excessive filename lengths are made safe.

Choose an output folder, name the recipe, and select **Save** or **Save and run**. The
recipe name, folder, outputs, and export area persist in `export-recipes.json` alongside
Schist's preferences. Selecting a saved recipe loads it for editing; **New recipe** starts
a separate one; **Delete recipe** removes only the preset. Save before switching recipes
to retain changes. Distinct presets must have distinct names.

Native exports run in the background, one source at a time. The status bar reports source
progress, files written, and failures; a failed output does not prevent later outputs.
The final status includes the first error if anything failed. Existing files, originals,
and edit sidecars are preserved: output bytes are staged in a temporary file and published
atomically without replacing an existing filename. A numeric suffix resolves collisions.

RGB profiles remain embedded. CMYK/Lab composites become sRGB and do not carry the original
native-color profile. PNG and TIFF retain up to 16 bits per channel; 32-bit source samples
are reduced to that supported encoding depth. JPEG and WebP use 8 bits. JPEG uses its
quality slider and composites transparency on white; the installed WebP encoder is
lossless and has no quality setting.

In the browser, presets persist in local storage and outputs become separate downloads
in the browser's download folder. Browser download permission and duplicate filename
handling apply. A browser may ask permission for multiple downloads. Encoding runs in
the browser and may briefly pause its UI for a large image. Native galleries are not
available in the browser.

The `export_recipes.lang` catalogs include translations for every shipped locale.
Catalog validation checks keys, placeholders, locale aliases, and font coverage.

Run `make check-export-recipes` for rendering, actual codec, depth, source preservation,
filename safety, gallery-sidecar, and partial-failure regression tests.

## Output finishing

Each output also saves its own finishing settings. Leave **Text watermark** empty to
omit a watermark. Enter text to shape it using Schist's font engine and available fonts;
choose black or white, opacity, and a corner or center position. Size is an em size as a
percentage of the final image's shorter edge. Long text wraps and scales down to fit.
Margins are 2.5% of the shorter edge. Font fallback can differ between machines; the
browser uses its loaded fonts. A watermark is flattened into exported pixels, never
added to the original document. Image-logo watermarks are not part of this workflow.

**Sharpening** applies a one-pixel-radius unsharp mask after resizing, from 0% (off) to
200%. It leaves alpha unchanged and weights the blur by alpha so transparent colors do
not create halos. The watermark is applied after sharpening.

**Convert to Profile** defaults to **Original**, preserving the existing RGB profile.
sRGB and Display P3 rewrite pixel values and embed the target ICC profile. **Browse…**
under this control imports a custom RGB ICC profile on native platforms (maximum 4 MiB).
The profile's bytes are saved in the recipe, so deleting its source file does not break
later runs. CMYK, Lab, grayscale, and invalid targets are rejected. Existing CMYK/Lab
source composites enter the finishing pipeline as sRGB. Conversion uses relative
colorimetric intent. Untagged RGB uses the application working profile captured when the
job starts; native CMYK/Lab composites use sRGB. The working profile is runtime context,
not a saved recipe setting. Browser recipes can use saved custom profiles, but the browser UI
currently offers only the built-in choices and already embedded profiles.

**Photo metadata** has two explicit allowlist choices: **None** (the backward-compatible
default) omits descriptive metadata; **Copyright** includes only copyright. Both omit
GPS, capture time, camera information, keywords, captions, thumbnails, and other EXIF/XMP
fields. ICC color profiles remain embedded independently of this descriptive metadata
setting. Enter a copyright value to override the source, or leave it blank to retain the
original capture's EXIF copyright or XMP sidecar rights. XMP rights take precedence,
including an explicit empty rights field. Embedded XMP rights in PNG, JPEG, WebP, and TIFF
also take precedence over EXIF copyright. Other source formats rely on readable EXIF or
an XMP sidecar. A malformed or ambiguous XMP sidecar fails
that copyright-preserving output instead of silently omitting rights. Outputs that strip
metadata or supply their own copyright can still succeed. Browser documents and new
untitled documents need an explicit copyright value because they have no readable
original path. Full EXIF/XMP preservation is intentionally unavailable.

Copyright is embedded as UTF-8 XMP `dc:rights` into the actual PNG, JPEG, WebP, or TIFF
file, without copying an opaque source packet. TIFF RGB source reads explicitly preserve
embedded ICC profiles so subsequent target-profile conversion uses the correct source color space. XML escaping preserves Unicode names and
prevents text from injecting extra properties. The encoder container adjustments follow the
[PNG iTXt specification](https://www.w3.org/TR/png-3/#11iTXt) and
[WebP container specification](https://developers.google.com/speed/webp/docs/riff_container).


Finishing settings are optional fields in the version-1 recipe library. Existing
recipes retain their previous behavior. New UI labels reuse existing catalogs except
`export_finishing.watermark`; its catalog files explicitly identify English fallbacks
pending translation review. Structural validation does not certify translation quality.
