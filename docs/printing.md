# Printing and contact sheets

Choose **File → Print document…** for the current document, including unsaved
edits. Select photographs in the gallery and choose **Contact sheet from
selection…** from File or the photograph context menu to print a sheet. Videos
are excluded. The selection is frozen when the dialog opens, in selection order. Contact
sheets use the native local gallery; browser builds print the active document.

Choose A4 or US Letter, portrait or landscape, margins from 5–30 mm, a grid up
to five columns and six rows, and 72/150/300/600 output DPI. Fit mode preserves
aspect ratio, centers each image and never crops. Each grid cell has a 2 mm inset and reserves room
for its caption. Actual size uses the source document's resolution
DPI: 300 pixels at 300 DPI occupy 25.4 mm. An image that cannot fit at actual
size produces an error rather than silently scaling or clipping it. Output DPI
controls image downsampling and caption quality, not physical image size; small
sources are not artificially upsampled.

The caption option prints filenames, five-star ratings and XMP captions.
Captions use Schist's Unicode text shaping and available font fallback at 8 pt;
install suitable script fonts if needed. Excessively long captions fail the job
with advice to reduce the grid or disable captions. No text is silently cut off.
Captions are raster images in the PDF, not searchable/selectable text. Native
font availability and browser-loaded fonts can differ.

**Save PDF…** saves a real, self-contained PDF 1.4 file. Desktop builds also
provide **Save and open PDF to print…**, which hands the saved PDF to the OS's
associated viewer. Use that viewer's Print command to choose a printer and its
settings. Print at **100% / actual size**, with fit-to-page disabled, to preserve
physical dimensions. A configured PDF viewer is required; if it does not open,
open the saved file manually. No shell command is constructed from a filename.
Browser builds download the PDF; open the download in a PDF viewer to print.
iOS and Android expose the save workflow and rely on the system's subsequent
PDF open/share/print support. Hardware and platform print dialogs are not
controlled or automatically submitted by Schist.

RGB image pixels are converted to sRGB with relative colorimetric intent before
being embedded with an sRGB ICCBased color space. Transparency is flattened onto
white. RGB-tagged input must contain a readable RGB profile; untagged RGB input uses
the current working RGB profile. CMYK/Lab documents require a valid matching
embedded profile and use the compositor's native-to-sRGB transform (perceptual
intent), with no second RGB conversion. Invalid or missing native profiles and
grayscale ICC profiles are rejected. Do not assign a different profile just to
bypass the check. Grayscale and indexed documents with RGB interpretation can print. Printer-specific ICC
conversion is delegated to the PDF viewer/driver to avoid double management;
there is no custom printer-profile selector, soft-proof simulation, or PDF/X
claim. Paper/ink gamut and a viewer's color-management implementation still
influence the final print.

Gallery photos use their existing edited PSD sidecar when present, while their
original filename, stored rating and XMP caption label the sheet. A sidecar
failure aborts the job rather than silently printing the original. Active
unsaved document edits are included by Print document; gallery sheets read the
saved gallery edits. Originals and sidecars are never modified. PDF save is
atomic and never replaces an existing file; choose a fresh name to repeat a job.
Cancel during preparation stops between compositing strips and images and
prevents output. Cancel the save picker to discard a completed PDF.

Jobs are limited to 200 photographs, 40 million source pixels per photograph,
32768 pixels per source edge, and 256 MiB of compressed object data. Processing decodes one source at a time,
composites in cancellable strips, and retains compressed PDF objects until save.
Caption rendering and individual decoder calls do not support mid-call cancel.
Native processing runs on the background executor; browser processing yields to
the event loop between strips so Cancel and painting remain responsive. Both
paths leave document history intact.

## Format and validation references

The implementation uses the PDF page tree, MediaBox, image XObjects, Flate
streams, cross-reference table, and ICCBased RGB color spaces. References:
[PDF Association graphics specification errata](https://pdf-issues.pdfa.org/32000-2-2020/clause08.html),
[PDF Association syntax specification errata](https://pdf-issues.pdfa.org/32000-2-2020/clause07.html),
and the [PDF/raster RGB image discussion](https://pdfa.org/wp-content/uploads/2017/07/PDFraster10.pdf).
The output is general PDF 1.4; it does not claim PDF/raster conformance.
No code or Adobe headers were taken from these references.

Run `make test-printing`, `make check-app`, `make check-app-web`,
`make lint-printing`, `make check-symmetry-format`, and `make check-i18n`. Tests independently parse generated output with lopdf and
cover pagination, physical geometry, ICC conversion and rejection, malicious
caption text, caption overflow, cancellation, limits and no-clobber saving.

New print strings live in `printing.lang` in every existing locale. Short labels
in German, French and Spanish have initial translations; remaining new strings
explicitly retain English fallback pending language review. Structural catalog
checks do not certify translation or font quality. No locales were added.
