# Editable filter stacks

Select a pixel layer or smart object, then use **Filter stack → Add filter**
in the Layers panel. Choose an effect, adjust its existing filter controls,
and press OK. Repeat to build a stack. Effects run from top to bottom.

Click an effect's name to reopen its saved parameters. Enable/Disable compares
its contribution, Up/Down changes order, and Remove deletes it. Each committed
change is one undoable edit. Cancel and Preview-off restore the exact previous
native pixels; the source is never progressively filtered. Removing every
filter restores the original source. **Bake stack** keeps the visible result
and removes the editable recipe; Undo restores both.

Stack effects apply to the layer's source across its canvas intersection,
independently of the current selection. Smart-object effects run in original
source coordinates, before its placement transform. They retain the original
unfiltered pixels in addition to the filtered smart-object source. RAW-backed
layers can use Camera Raw as a raster stack effect; the ordinary Camera Raw
command still develops the original capture separately.

Only self-contained, in-process filters appear in the chooser. Filters that
require a displacement map, path, backdrop, or external dialog cannot yet be
represented in a saved stack. Each effect retains its own foreground and
background colors, so replay does not depend on the current swatches. Missing
filters keep the last rendered result and the complete source/recipe; editing
cannot partially commit a failed render. Disable or remove a missing effect
to continue rendering the others.

Painting, destructive filters, moving, transforming, rasterizing a smart object,
and color-mode conversion bake the stack as part of the same undoable operation.
Smart-object transforms keep their filtered source, so baking does not remove
the effect's appearance. Layer duplication preserves the stack. Cross-document
insertion into a different native color mode bakes it. Masks, opacity, blend
modes and layer styles compose with the filtered raster normally. Convert text
or shape layers to pixel layers or smart objects before adding a stack.

PSD and PSB save both the visible layer raster and Schist-owned `ScFs` (recipe)
and `ScFo` (lossless source) additional-layer-info blocks. The source stores
sparse native tiles with their original u8/u16/f32 samples, including CMYK/Lab
channels and transparent colors. Reopening in Schist keeps editing available.
Other applications see the rendered pixels; this is **not Adobe Smart Filter
metadata**. Applications that strip unknown PSD blocks also discard the recipe.
Flat image exports contain the visible result only. Native crash recovery and
shared-document checkpoints retain both blocks. Uncommitted stack previews are
excluded from recovery and cloud edits.

Sources are limited to 512 MiB before compression. Rendering also bounds image
area and tile coverage to reject corrupt files and pathological narrow images
before allocation. Large filters currently use the same synchronous CPU fallback
as ordinary native filters, including in the browser; expensive stacks can pause
the UI while rendering. Source metadata is counted toward the undo memory limit.

All strings use `schist-i18n`. The new `filter_stack.lang` catalog exists in
every shipped locale with an explicitly marked English fallback pending
translation review; no new locales were added.

Run `make check-filter-stacks`, `make check-app`, and `make check-i18n` to validate
native source fidelity, replay, ordering, enable/disable, undo/bake, PSD/PSB,
shared checkpoints, recovery snapshots, smart-object placement and catalog wiring.
