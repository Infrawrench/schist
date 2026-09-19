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

The new `export_recipes.lang` catalogs currently use English fallback text in non-English
locales; locale coverage is structurally complete and translations remain to be supplied.

Run `make check-export-recipes` for rendering, actual codec, depth, source preservation,
filename safety, gallery-sidecar, and partial-failure regression tests.
