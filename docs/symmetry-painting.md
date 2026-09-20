# Symmetry and seamless painting

Select **Brush**, **Pencil** or **Eraser**, then open **Symmetry** in the tool
options bar, beside **Preset**.

- **Vertical** mirrors each dab across a vertical axis; **Horizontal** mirrors
  across a horizontal axis.
- **Radial** rotates the stroke around the center. Its slider chooses any count
  from 2 through 24 copies, including the original stroke.
- **Position** closes the popover so the next canvas click or drag places the
  axis/center. Release returns to painting. **Escape** restores its previous
  position. **Center** returns it to the canvas center.
- **None** returns to ordinary painting.

The axes and center appear on the canvas, and the brush outlines show each
copy. Positioning uses document coordinates, including when the view is rotated
or zoomed. The center is stored as a fraction of the canvas, so switching
between different document sizes keeps it in the same relative place. These
controls are session settings, not document content or saved brush recipes.

Pressure, size, hardness, texture, scatter, spacing and smoothing still apply.
Texture and scatter are reflected/rotated with the stroke. Every copy shares
one stroke coverage buffer: crossing the center or another copy does not build
past the selected opacity. A completed stroke is one undo step. Escape or
switching tools before releasing cancels the entire stroke. Selections clip
copies at their destination; layer masks still determine their visible result.
Retouch tools retain their normal behavior.

## Seamless tile preview

Enable **Seamless tile preview** in the same popover to repeat the canvas in
both directions. Zoom out to inspect the surrounding repeats; the outline
marks the original canvas, which remains the export area. The preview respects
layer masks and display color management. Fractional zoom and rotation sample
across the repeated edges, avoiding artificial transparent seams.

Brush, Pencil and Eraser can paint in any visible repetition. Dabs crossing an
edge continue at the opposite edge; corners wrap in both directions. Drag
continuously across the visible edge without jumping the pointer to the other
side. Selection coverage is evaluated at the wrapped destination. Symmetry
uses the source canvas coordinates, so a dab in another repetition produces
the same pattern. Position mode likewise places the center in the source canvas.

Turning the preview off returns to the usual canvas view. The repeating view
does not resize the document, duplicate its layers, change export dimensions,
or make an existing pattern seamless automatically: it lets you see and paint
across its seams. Other editing tools keep their ordinary document coordinates;
wraparound editing is supported by Brush, Pencil and Eraser.

The periodic resampler currently runs on the CPU on native and browser builds.
Only source tiles needed by the view are composited, and a bounded sparse tile
index handles views crossing opposite edges. Very large source documents and
very distant zoom levels can still be expensive; this workflow is most useful
for texture-sized documents.

Run `make test-symmetry` for geometry, brush/eraser/pencil, pressure, undo,
cancellation, mask/selection and periodic resampling tests. Run
`make check-symmetry` for the editor integration and `make check-i18n` for
catalogs, aliases, browser loaders and font coverage.
