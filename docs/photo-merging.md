# Photo merging

On native builds, open the source photos in separate document tabs, then choose
**Image → Merge Photos…**. Select between two and sixteen documents. The dialog
uses snapshots of the process-color composites of visible layers, including
unsaved edits. Editing overlays, print-separation views and overprint previews
are excluded. A successful operation opens a new, unsaved, 32-bit RGB document.
All input tabs and their undo
histories remain intact. Closing the dialog or pressing Cancel abandons the job.
Compositing, registration and output construction run on the background executor.

The browser build currently has no merge dialog. The independent processing crate
has no native dependencies, but the editor integration requires a worker thread.

## Workflows

* **Automatic alignment** creates one raster layer per selected document, using
  that document's name. Translation remains the default; enable **Perspective
  alignment (includes rotation)** for modest rotation and viewpoint changes.
  Leave the crop option off to retain the union of the input canvases, or crop to
  their common rectangular area. Source documents with multiple layers are
  flattened in the result; their original editable tabs remain available.
* **Focus stacking** aligns to the first selected document, computes an absolute
  luminance Laplacian, averages the measure over a local window, and selects the
  sharpest source at each output pixel. Radius controls that local window. Turn
  alignment off for already registered photos or texture-poor sequences. Hard
  focus selection can show seams, noise or halos near depth discontinuities;
  the result contains every registered source on its own layer with an editable
  native layer mask. Select a mask thumbnail in Layers and paint to correct
  selection boundaries. To substitute a different source, reveal its mask and
  hide the competing source at that position; ordinary layer order and alpha
  blending apply after edits. Masks are one-hot (only one source is revealed per
  pixel), so the initial layered composite reproduces the focus selection even
  with partially transparent inputs. Mask edits use the existing undo history
  and survive layered PSD/PSB saves; a flattened export loses editability.
  Projective alignment can compensate for modest global scale changes, but
  does not model depth-dependent lens breathing.
* **Bracketed HDR** converts sRGB samples to linear light, divides by the supplied
  relative exposure and combines the estimates with triangular exposure weights.
  Set each input's EV with the minus/plus buttons (one-third-stop increments).
  For exposures of 1/250, 1/125 and 1/60 second at fixed aperture/ISO, use roughly
  −1, 0 and +1 EV. A positive offset means more light reached the sensor. The
  values are entered explicitly; EXIF exposure is not inferred from an edited
  document. Clipped samples receive zero weight when another exposure contains
  useful data. Fully clipped brackets cannot recover missing detail.
* **Panorama stitching** registers each photo against the previous selected
  document, in tab order, and builds a canvas containing the resulting mosaic.
  Consecutive photos need at least 25% overlapping area, measured against the
  smaller canvas. Arrange/open the tabs in the desired sequence before merging.
  Enable **Cylindrical panorama** for upright yaw sweeps, and set **Focal length /
  image width** to focal length in pixels divided by the image width in pixels
  (equivalently, physical focal length divided by the sensor width for an
  uncropped image). The default 1.0 is a starting value, not an EXIF estimate.
  All selected views must share focal length and pixel dimensions.
  Overlaps are feathered in linear light; Feather controls the distance from
  each image edge over which its weight increases. Empty areas stay transparent.

HDR tone mapping is enabled initially. It uses a global, luminance-based
Reinhard curve with an adjustable display exposure, followed by sRGB encoding.
Disable **Tone mapping** to retain extended-range HDR values in the 32-bit output.
Those stored values are **sRGB-transfer-encoded scene radiance**, not scene-linear
samples: decoding the transfer curve recovers the exposure-normalized radiance.
Values above 1 remain in the document and in layered PSD/PSB saves; ordinary
display and 8/16-bit exports can clip them. HDR tests cover both the recovered
radiance and the tone-mapped image. This is exposure-normalized radiance merging,
not exposure fusion; it assumes the input response is sRGB after color conversion
and does not estimate a camera response curve. See the
[OpenCV HDR overview](https://docs.opencv.org/4.12.0/d2/df0/tutorial_py_hdr.html)
for the distinction between exposure inputs, HDR radiance and tone mapping.

## Registration and limits

The default translation registration uses area-averaged luminance pyramids and zero-mean normalized
cross-correlation. It searches the coarse scale exhaustively, retains eight
candidates, and refines to integer-pixel offsets. Stack alignment needs 60%
overlap; panoramas need 25%. Transparent samples are excluded. Low-texture,
non-overlapping and low-correlation pairs produce an error instead of arbitrary
placement. Highly repetitive patterns can still produce an ambiguous match.

**Perspective alignment** fits an eight-parameter homography against the first
selected photo using zero-mean normalized correlation and bounded coordinate
descent, with at most roughly 96 × 96 samples per candidate and a second
sampling grid for validation. Fine repeating details can alias on these grids. Initial rotation hypotheses span −15° to +15° in 3° steps. It estimates
rotation, translation, scale, shear and projective terms; resampling uses
premultiplied-alpha bilinear interpolation. At least 60% overlap and a final
correlation of 0.75 are required. This is a local optimizer, intended for modest
viewpoint changes of a textured approximately planar subject, or camera rotation
about its optical center. It does not normalize a 90° orientation mismatch,
solve arbitrary viewpoints or guarantee convergence. Highly repetitive texture
can remain ambiguous. Translation registration rejects competing placements
with essentially equal scores when they survive its candidate beam. With crop
enabled, projective stacks use the largest integer rectangle contained in all
transformed image footprints; existing transparent source pixels remain
transparent. Without crop, the union retains transparent corners.

**Cylindrical panorama** inverse-projects each rectilinear image onto a cylinder,
then registers consecutive projected photos by translation. It assumes an
upright camera, square pixels, a centered principal point, a fixed focal length,
negligible lens distortion, and yaw about
the optical center; pitch, roll, parallax and moving panorama subjects can create
seams. Projective stack alignment and cylindrical panoramas are separate choices.
This is an open strip, not a wraparound 360° panorama: the first and last photos
are not joined. Sequential registration still uses integer translations and may
accumulate drift; it does not optimize all cameras jointly. Feathering still
uses the source canvas edge, and is not a
content-aware seam finder or multiband blend. Sources should have the same pixel
scale; mismatched cylindrical input dimensions are rejected.

**Remove moving-subject ghosts** in HDR compares exposure-normalized channel
radiance against the first selected photo, measured on that capture’s exposure
scale so adding a common EV offset does not change motion detection. A discrepancy
above an absolute 0.02
plus 25% of the brighter estimate uses only that reference photo at the pixel.
Clipped channels (outside 0.03…0.97 encoded sRGB) do not establish motion. Static
pixels retain the original weighted radiance merge. This removes detectable
moving copies while anchoring the subject to its position in the first photo;
choose a suitable reference by arranging the source tabs. Hard switching can
create boundaries, noise or lose dynamic range. Motion visible only in clipped
samples, camera-response differences and imperfect alignment can escape or
trigger this heuristic. It is optional and disabled by default. There is no
optical flow, lens calibration or automatic exposure balancing.

Inputs may differ in dimensions. Each input and the output are limited to
64 million pixels and 30,000 pixels per side, with at most 128 million input
pixels in total. The editor checks the aggregate budget before flattening input
snapshots; the engine checks the output bounds before allocating its canvas.
Large allocations use fallible reservation where possible. Working memory can
still reach several gigabytes near these limits, in addition to open documents;
reduce source dimensions or the number of inputs on memory-constrained devices.
Samples must be finite and in the input sRGB range 0…1. RGB working profiles are
converted to sRGB; native CMYK/Lab composites already enter the RGB pipeline.

Cancellation is checked between composite strips, registration candidates,
focus-map rows, output rows and layer-construction strips. No partly completed
document is installed, and a late result from a replaced dialog is discarded.

## Verification and translations

`make test-photo-merge` runs synthetic tests for signed displacement and differing
dimensions, exposure-invariant registration, three-photo panorama reconstruction,
focus selection, radiance recovery from clipped brackets, tone mapping,
transparency, invalid input and cancellation. `make test-photo-merge-editor`
checks result construction, cropped source pixels, abandoned jobs and HDR
document/PSD round-tripping, editable focus-mask recomposition and mask saves.
Advanced engine fixtures additionally cover non-affine homography coordinate
recovery, rotation, common-support cropping with fractional alpha, a cylindrical
yaw sweep and its overlap seam, moving HDR objects and periodic-pattern rejection. `make check-photo-merge` checks the editor integration.

The shipped catalogs include the original dialog's strings and the generic Norwegian
and Serbo-Croatian aliases are synchronized. Twenty major-language translations
were directly authored; most other catalogs used translation drafts with
photography-specific terminology review. The unsupported translation-service
languages were authored directly. Native-speaker review has not been performed;
technical phrasing in lower-resource languages, particularly Pāli, Cornish,
Marshallese, Chamorro, Northern Sami and Dzongkha, remains less certain.
Structural/catalog and font checks do not certify linguistic quality.

### Advanced control translations

The new advanced controls have English strings and draft translations in German,
French, Spanish, Italian, Portuguese, Dutch, Swedish, Danish, Bokmål (also the
Norwegian alias), Polish, Czech, Russian, Ukrainian, Japanese, Simplified Chinese,
Korean and Finnish. Other existing catalogs explicitly mark these keys as English
fallback pending translation. No locales were added. Neither these drafts nor
the English fallbacks are claimed to be native-speaker reviewed.
