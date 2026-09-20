# Recordable actions

Open **Edit → Actions…**, choose **Start recording**, and make edits in the
current document. The status bar shows that an action is recording. Open
**Actions…** again, choose **Stop recording**, enter a name, then **Save action**.
Recordings can contain up to 128 steps. Switching documents stops capture when
the next supported edit commits and keeps the steps already recorded.

Select a saved action to edit it. Choose a step to change its numeric parameters,
move it up or down, or remove it. Fill commands include their recorded colour
and opacity. Adjustment steps preserve their complete settings, including
curves and imported channel settings; the manager exposes the adjustment's
existing numeric controls. Settings without numeric controls remain preserved
for replay. **Save action** saves changes and renames the selected action.
**Delete action** removes that saved action.

**Replay on active document** applies the working action to the current active
layer and selection. Layer creation, duplication and adjustment insertion move
the active layer as they do during normal editing. Actions use semantic command
and filter identifiers, never recorded mouse coordinates or document-specific
layer ids. Selecting one layer in the Layers panel, its context menu,
Spotlight, or Move tool auto-selection records its exact name. Replay searches
the whole tree, including closed groups, and requires a unique match. Selecting
ambiguous names or multiple layers stops recording and retains earlier steps,
so later edits cannot silently target the wrong layer. Rename layers to make
their targets unambiguous; you can edit the recorded name in the action manager.

Supported recording operations are:

- Select All, Deselect, Inverse; foreground and background fills.
- New Layer, Duplicate Layer, Rasterize Layer, Flatten, Merge Down, Merge Visible.
- Gaussian Blur, Box Blur, Motion Blur, Sharpen, Unsharp Mask and Median.
- Add adjustment layers, commit settings on the active adjustment layer, and
  apply destructive adjustments to RGB/grayscale pixels.
- Free Transform and Transform Selection, committed with Enter or by changing
  tools. Scale and rotation are relative to the target content/selection centre;
  X/Y translation is a percentage of the target canvas width/height. The manager
  edits scale, rotation, offsets and interpolation. Recorded values contain no
  pointer coordinates or source dimensions. Cancellation adds no step. While
  recording, choosing another layer or starting another supported edit commits
  the pending transform first, preserving the actual order and target. Opening
  the Actions manager also commits a pending transform before capture/replay.
- Camera Raw development of RAW-backed layers. All 15 numeric settings are
  editable, and each replay develops that target layer's own original capture.
  Layers without a retained RAW source, or whose developed dimensions no longer
  match the canvas, fail transactionally. The original capture stays embedded.
- Editable filter-stack insertion and committed settings; enable/disable,
  remove, move up/down, and bake. Entries retain their full parameters, enabled
  state and foreground/background colours. Existing entries are addressed by
  stack position plus expected filter ID, never by document IDs. Positions
  shown in the manager start at one. A mismatched stack fails transactionally.
  Missing installed filters can still be disabled or removed from a stack.

Filter previews and cancelled filter dialogs do not add steps. Creating an
adjustment layer is an edit even if its subsequent settings dialog is cancelled;
the action keeps the newly created layer's default settings. Committing that
dialog folds its settings into the immediately preceding insertion. Unchanged
settings dialogs do not add steps.

Brush strokes, multi-layer selection, history navigation, clipboard commands,
deleting layers, file/application commands, destructive random/neural filters,
external plug-ins and filters needing a map, backdrop or path are not recorded.
Self-contained stack filters use their saved parameters; an inherently random
stack filter can produce different pixels on successive renders. Destructive
Camera Raw on an ordinary raster layer remains outside recording. Native-channel
edits and destructive CMYK/Lab adjustments are not recorded; use an adjustment
layer for the latter. Unsupported dispatched operations report their omission
in the status bar. Unsupported operations never become executable steps in an
action file.

Replay validates the entire action's format, eligible operations, installed
filters and parameter ranges before starting. A missing layer, incompatible
adjustment, locked pixel layer, empty pixel region, refused/no-op command or
filter error fails with its step number. All previous steps in that replay roll
back, preserving the original undo/redo history, save point, selection and
Reselect state. Successful replay groups all document edits into one undo entry.
Layer focus follows normal panel selection behaviour and is not itself an undo
operation; an action that only changes layer focus creates no document edit. Replay does not
recursively record itself.

## Gallery copies

Select photos in the gallery, then open **Gallery → Actions…** and choose an
action. **Replay on selected gallery photos** asks for an output folder. Each
photo is processed on a background worker from its existing saved PSD edit when
one exists, otherwise from the original. The saved active layer is used, falling
back to the topmost pixel layer when the loaded document has no active layer.
Each file starts with independent editor state.

Results are new layered PSD files named `photo-action.psd`,
`photo-action-2.psd`, and so on. A temporary file and an atomic no-clobber
commit prevent partial files and collisions, including simultaneous writers.
Originals, existing sidecars, and earlier output copies are never overwritten.
A failed photo produces no output; other photos continue. The results dialog
lists each output path and each failed input with its reason. **Stop after
current photo** stops before the next input and retains completed copies.

## Storage and validation

Native installations save schema-versioned JSON in
`$XDG_CONFIG_HOME/schist/actions.json` (or `$HOME/.config/schist/actions.json`).
Browser builds use local storage under `schist.actions.v1`. Saving the native
library uses a temporary file and atomic replacement. Load/save errors appear
in the status bar. Action libraries are limited to 256 actions and 4 MiB.

The action manager uses `schist-i18n`, reusing the translated layer, transform,
Camera Raw and filter-stack controls. No locales were added. Catalog validation
checks keys, placeholders, locale aliases, and font coverage.

Libraries saved by this version use schema 2. Existing schema 1 libraries load
without changes to their steps and upgrade on the next save. Both retain the
same size, action-count and step-count limits; non-finite values, singular or
excessive transforms, invalid RAW settings and out-of-range stack positions are
rejected before replay. Browser storage keeps its existing key so old libraries
remain discoverable.

Run `make check-extended-actions` for semantic replay, single-step undo,
rollback and redo preservation, transform commit/cancel recording, relative
transforms across different canvas sizes, nested/ambiguous named layers,
RAW source development, stack operations and missing effects, version migration,
parameter/schema validation, filter errors, and gallery copy/sidecar safety.
Run `make check-i18n` for catalog validation.
