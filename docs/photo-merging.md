# Photo merging

On native builds, open the source photos in separate document tabs, then choose
**Image → Merge Photos…**. Select between two and sixteen documents. The dialog
uses snapshots of their visible composites, including unsaved edits; a successful
operation opens a new, unsaved, 32-bit RGB document. All input tabs and their undo
histories remain intact. Closing the dialog or pressing Cancel abandons the job.
Compositing, registration and output construction run on the background executor.

The browser build currently has no merge dialog. The independent processing crate
has no native dependencies, but the editor integration requires a worker thread.

## Workflows

* **Automatic alignment** creates one raster layer per selected document, using
  that document's name. It compensates for horizontal and vertical displacement.
  Leave the crop option off to retain the union of the input canvases, or crop to
  their common rectangular area. Source documents with multiple layers are
  flattened in the result; their original editable tabs remain available.
* **Focus stacking** aligns to the first selected document, computes an absolute
  luminance Laplacian, averages the measure over a local window, and selects the
  sharpest source at each output pixel. Radius controls that local window. Turn
  alignment off for already registered photos or texture-poor sequences. Hard
  focus selection can show seams, noise or halos near depth discontinuities;
  this version does not model lens breathing or produce editable focus masks.
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

Registration uses area-averaged luminance pyramids and zero-mean normalized
cross-correlation. It searches the coarse scale exhaustively, retains eight
candidates, and refines to integer-pixel offsets. Stack alignment needs 60%
overlap; panoramas need 25%. Transparent samples are excluded. Low-texture,
non-overlapping and low-correlation pairs produce an error instead of arbitrary
placement. Highly repetitive patterns can still produce an ambiguous match.

The supported geometric model is translation only. There is no rotation,
perspective, cylindrical/spherical projection, lens distortion correction,
subpixel resampling, exposure balancing or moving-subject deghosting. Panorama
stitching is therefore intended for translated/approximately planar overlapping
images; ordinary wide-angle camera rotations often require a different model.
Alignment cannot compensate for focus breathing or moving objects in a bracket.

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
document/PSD round-tripping. `make check-photo-merge` checks the editor integration.

All 150 shipped catalogs include the dialog's strings and the generic Norwegian
and Serbo-Croatian aliases are synchronized. Twenty major-language translations
were directly authored; most other catalogs used translation drafts with
photography-specific terminology review. The unsupported translation-service
languages were authored directly. Native-speaker review has not been performed;
technical phrasing in lower-resource languages, particularly Pāli, Cornish,
Marshallese, Chamorro, Northern Sami and Dzongkha, remains less certain.
Structural/catalog and font checks do not certify linguistic quality.
