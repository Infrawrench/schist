# Automatic lens-profile correction

Open a photo or develop a RAW file, then choose **Filter → Lens Correction**.
Schist matches the photo's EXIF camera maker/model, lens model, and unrounded
focal length against an installed Lensfun database. A unique compatible match
loads its calibrated distortion and lateral chromatic-aberration correction;
calibrated vignetting also needs aperture, focus distance, and supported color
context. The dialog shows the selected lens. No fuzzy name guesses are made.
Missing/ambiguous metadata, incompatible mounts, and missing calibration leave
manual correction available. Opening or cancelling the dialog does not change
the source photo. Preview and Apply use the ordinary filter undo workflow.

Use **Lens profile** to override the match, and edit focal length, camera crop
factor, aperture, or focus distance. A value of zero means unknown for aperture
and distance. Most distant-focus Lensfun calibrations use `1000` metres; enter
this only when it describes your shot. Select **Manual correction only** to
turn off the calibrated pass. The existing distortion, color-fringe, vignetting,
perspective, angle, and scale controls remain available after calibration.
**Match photo metadata** resets the selection from EXIF. Changing a capture
setting recalculates the selected profile; unavailable settings disable it
instead of retaining the previous shot's calibration.

Calibration requires the original full frame, with its optical center at the
image center. Develop first, apply lens correction next, and crop/transform
last. Automatic selection is disabled for an active pixel selection or a layer
whose filter region is smaller than the canvas. Schist cannot infer an earlier
crop from an ordinary flattened file. Manually selecting a profile on a crop,
montage, shifted layer, or previously corrected JPEG can produce wrong results.
Borders outside the source become transparent; use the existing scale or crop
controls to trim them.

For editable persistence, add **Lens Correction** in the layer's filter stack
and save PSD/PSB. The recipe stores the actual coefficients, focal length,
camera/calibration crop factors, and aperture/distance values. Reopening,
previewing, undoing, and exporting do not require the database or its former
ordering. A missing database is displayed as **Saved calibration**. Reimport
the database to change the focal length or profile of such a saved recipe.
A normal destructive Apply stores corrected pixels with undo, not an editable
recipe. RAW capture bytes remain intact; re-developing RAW pixels after a
destructive correction can replace that correction. Prefer the filter stack.

## Getting a database

Schist does not download profiles automatically and does not ship a production
Lensfun database or a native Lensfun library. Use Lensfun's official database:
https://github.com/lensfun/lensfun/tree/master/data/db

- Linux: install your distribution's Lensfun data package. Schist searches
  `/usr/share/lensfun/version_1`, `/usr/share/lensfun`, and
  `/usr/local/share/lensfun/version_1`.
- macOS: `/opt/homebrew/share/lensfun/version_1` is also searched.
- Native builds on any platform: set `SCHIST_LENSFUN_DIR` to a directory
  containing XML files before starting Schist. This is also the straightforward
  Windows route. Files in this directory take priority over system profiles.
- User updates at `~/.local/share/lensfun/updates/version_1` are checked before
  system locations.
- In the dialog, **Import Lensfun XML…** adds one chosen camera/lens database
  file to the current session. Import both camera and lens files if they are
  separate. Then select the lens or choose **Match photo metadata**. Place
  those files in `SCHIST_LENSFUN_DIR` to retain them between sessions.
- Browser builds have no installed-directory discovery. Use XML import. Saved
  recipes still replay without importing again. Correction uses the CPU fallback.

Imports parse on a background worker, never change the imported file, and do
not overwrite existing same-identity profiles in this session. Cancel the file
picker to leave everything unchanged. An import already parsing can finish
adding profiles after the dialog closes; it cannot modify the photo. Each XML
file is limited to 8 MiB; installed database discovery has a 64 MiB total budget
and at most 4096 XML entries per directory. DTDs and external entities are not
accepted. Restart after replacing installed calibration files.

## Models and color handling

The reader supports Lensfun XML database versions 1 and 2, centered rectilinear
lenses, and calibration entries without their own crop/aspect attributes:

| Correction | Supported models | Interpolation |
| --- | --- | --- |
| Distortion | `poly3`, `poly5`, `ptlens` | Linear coefficients between bracketing focal lengths of the same model |
| Lateral chromatic aberration | `linear`, `poly3` | Linear coefficients between bracketing focal lengths |
| Vignetting | `pa` | Up to eight neighbors, inverse squared distance in normalized focal length, aperture, and reciprocal focus distance |

Measured sample values are reproduced exactly. No focal/aperture/distance
extrapolation is performed. Interpolation is Schist's documented conservative
subset, not Lensfun's spline algorithm; results between samples can differ from
other Lensfun applications. Fisheye/projection conversion, decentered lenses,
ACM models, automatic crop, and calibration-specific sensor attributes are not
supported. Unsupported models are omitted, not interpreted as another model.
The camera mount must match the lens mount or its declared compatibility list;
when several sensor calibrations exist, the closest non-larger calibration
sensor crop factor wins only if unique.

Geometry/TCA follow Lensfun's pixel-center convention and sensor-diagonal crop
normalization. Corrections are combined into one bilinear sample per channel,
with premultiplied alpha; alpha follows green. Vignetting is corrected before
resampling with gain `1 / (1 + k1*r² + k2*r⁴ + k3*r⁶)`, where `r = 1` at the
calibration sensor corner. RGB is decoded from sRGB to linear light for this
multiplication and encoded afterward. Alpha is unchanged by the gain, highlights
are not clipped in the float calculation, amplification is bounded to 16×, and
invalid/near-zero denominators receive neutral gain. Apply calibrated vignetting
before nonlinear tone edits; inverse sRGB encoding does not undo an earlier
camera/RAW tone curve.

Calibrated vignetting is supported only for **RGB documents explicitly tagged
with Schist's built-in sRGB profile**. The dialog offers **Convert to Profile…**
with sRGB selected, cancelling the current lens preview before opening the
ordinary conversion dialog. Convert first and reopen Lens Correction. This
preserves the appearance of Display P3 and other working/source spaces. If the
pixels are already known to be sRGB (including Schist's developed RAW output),
**Assign Profile → sRGB** is sufficient; do not assign sRGB to arbitrary RGB
pixels because assignment changes their interpretation. Convert/assign before
building a persistent lens-correction stack.

Untagged input, other RGB profiles, grayscale, Lab, and CMYK keep calibrated PA
disabled. The native filter entry point enforces this again during recipe
replay. Recognition compares the built-in ICC bytes, ignoring only timestamp
and profile-ID header bytes; unrelated external sRGB variants are conservatively
refused. Convert to the built-in sRGB profile to use them. This restriction
ensures replay is independent of the current working-space setting. Geometry/TCA
and existing manual vignetting remain available in other contexts.

## Sources, license, and validation

The equations and coordinate conventions are documented by Lensfun:

- https://lensfun.github.io/manual/v0.3.2/group__Lens.html
- https://lensfun.github.io/manual/v0.3.2/elem_calibration.html
- https://lensfun.github.io/manual/v0.3.2/corrections.html
- https://github.com/lensfun/lensfun/blob/v0.3.4/libs/lensfun/modifier.cpp
- https://github.com/lensfun/lensfun/blob/v0.3.4/libs/lensfun/mod-color.cpp

This is an independent Rust implementation of these mathematical models.
Lensfun's database is CC BY-SA 3.0. Users retain that license and attribution
when redistributing profile XML. A tiny unchanged camera/lens subset is included
only as regression data; its attribution and license are in
`plugins/filters-core/tests/fixtures/README.md`.

`make test-lens-profiles` checks real profile matching, measured coefficients,
interpolation/bounds, unsafe XML rejection, neutral identity, analytic geometry,
linear-light vignetting/alpha, finite amplification, recipe reconstruction,
and native color-context gating. `make check-lens-profiles-mcp` compiles the
headless integration; `make test-lens-profiles-mcp` verifies that its RGB route
enforces the same ICC prerequisite. `make check-lens-profiles`, `make check-app-web`,
and `make check-i18n` cover integration and catalogs. New wording is initially
an explicitly marked English fallback in non-English catalogs, pending review.
