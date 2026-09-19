# Refine Mask

Choose **Select → Refine Mask…** with a raster layer active. An active selection
seeds the mask; when there is no selection, the layer's existing mask is used,
including a disabled mask. The layer must be unlocked.

The dialog previews the isolated layer against a checkerboard, black, white or
magenta background. **Mask only** shows the coverage as grayscale. **Show
original mask** compares the seed with the adjusted result. Preview zoom enlarges
the image, with scrolling available for inspecting different parts.

The controls act in this order:

1. **Smooth** removes small irregularities with a local average and a narrowed
   coverage transition.
2. **Edge radius** sets the neighborhood for finding foreground and background
   colors. **Edge refinement** blends the current mask with coverage estimated
   from those colors. This can recover partially covered hair or fur near a
   rough selection where the foreground differs from its background. It is a
   local color estimate, not an AI segmentation tool. Similar foreground and
   background colors retain the seed; distant interiors and exteriors are kept.
3. **Shift edge** expands (positive) or contracts (negative) coverage in pixels.
4. **Feather** softens the final coverage transition.

**Remove color halos** replaces fringe colors with nearby confident foreground
colors, in proportion to the removed coverage. It requires an RGB raster and a
nonzero edge radius. It does not change the source pixel's alpha. Invisible RGB
samples do not influence the color estimate. This control is unavailable for
native CMYK and Lab layers; their masks can still be refined without converting
or changing the underlying native channels.

**Apply mask** writes a nondestructive layer mask, replacing any previous mask
on the active layer. The current selection remains unchanged. With color halo
removal enabled, Apply instead inserts a raster copy above the source, gives the
copy the corrected colors and mask, and hides the original. The original layer's
pixels, source data and mask remain intact. The copy retains the document's color
depth, blend properties and layer effects, but becomes a raster layer rather
than retaining editable smart-object, vector or camera-raw source data. A single
Undo restores the old mask or removes the copy and restores original visibility.

Previewing never writes to the document. Cancel, Escape and closing/replacing
the dialog drop the session. Native full-resolution processing runs in a worker;
cancelling while it is running discards its eventual result. The commit checks
the document revision, active layer and session so a stale result cannot replace
later work. Browser processing uses the same bounded blocks and yields to input between them,
so cancellation also stops a browser job before the next block.

The preview is sampled at a maximum of 720 pixels on its longest side; zoom
enlarges that preview rather than requesting more source detail. Radii are given
in document pixels and are scaled for the preview, so sub-preview-pixel details
are best judged in the full-resolution output. Apply processes the full image in
512-pixel blocks with overlapping context, bounding temporary memory and avoiding
block seams even when several radius controls are combined. All-zero output tiles
are omitted. Mask output is capped at 256 MiB of stored tiles to avoid excessive
allocations on unusually large documents.

Validation: `make check-mask-refinement` runs the core regression tests and
type-checks the editor and its tests. Tests cover soft edge estimation, ambiguous
colors, transparent RGB, smoothing/shift/feather boundaries, chunk seams, exact
source preservation, cancellation before and after preparation, mask and copy
undo/redo, disabled mask input, locked layers and stale sessions.
The same target checks undo memory accounting for masks and layer snapshots,
including inserted and removed raster layers and their nested source payloads.
These edits participate in the history memory budget so repeated refinements
cannot retain full-resolution masks or corrected layer copies outside that budget.

All controls use `crates/i18n`. `mask_refine.lang` exists in every shipped locale.
English, Swedish, German, French and Spanish are supplied; the other 145
catalogs currently contain explicitly marked English fallbacks pending
translation review. No locale has been added.
