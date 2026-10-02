# Design Mode

Design Mode is the page layout editor: a second body in the same shell as
the photo editor, sharing the window, the panels and the i18n but nothing
else that matters.

It ships **dark**, behind the default-off `design-mode` flag. IDML open/save,
text and shape authoring, placed graphics, relinking, page import, typography
controls and ten layout panels are implemented. Pen and curve editing, Story
Editor, thread editing, navigation tools, guides and output UI are implemented.
Interchange fidelity and external validation remain in roadmap item 9.

## Workspace and controls

Enable `design-mode` through `SCHIST_FEATURE_FLAGS='{"design-mode":true}'`
when launching the local build, then choose **View → Workspaces → Design**.
Choosing that workspace also leaves the gallery. The default feature flag stays
off; a development launch does not enable it for other installations.

Design uses a compact control bar above its document tab, a narrow tool column,
and one expanded panel beside a collapsible icon rail. Related panels share tabs:
Pages/Layers/Links, Properties/Character/Paragraph, Styles/Swatches and
Stories/Preflight. The active panel and collapsed state survive saved workspaces;
Design panel visibility is available in the workspace dialog.

The control bar shows page width/height without a selection and object X/Y/width/
height with a selection. Properties shows the selected frame's relevant settings.
Character and Paragraph retain their existing named-style editing semantics:
the named style is shown explicitly. Everyday settings are visible; appearance,
advanced typography, decorations, lists and style operations expand on demand.
Page setup and object-style details also start folded. Switching panels or closing
a section commits a focused field against its captured target before hiding it.

The Pages thumbnails are schematic frame bounds, clipped to each page and
normalized to its origin; they do not rasterize page artwork. The document tab
shows the layout filename and unsaved state, and closing it uses the existing
Save/Discard/Cancel transition.

## Why a separate body, and not a raster document with pages

A layout document is not a raster document with extra metadata. It has
styles, stories, threads, grids, inks and parent pages, none of which are
pixels. Bolting that onto `schist_core::Document` would put a page layout
engine inside an image editor's data model, and the two would end up
fighting over the same fields — the same `layers` meaning z-ordering in
one and a flat pass-through stack in the other, the same `links` meaning
smart objects in one and placed graphics in the other.

So the document is separate. `schist_layout::LayoutDocument` is its own
type, and it reaches the raster world only through linked `GraphicFrame`s,
which is the same relationship InDesign has: a placed image is a link plus
a frame, and the pixels stay where they were.

The cost is that the photo editor's `ToolPlugin`/`ToolCtx` cannot be
reused — they are hard-bound to `schist_core::Document`. Design Mode's
tools are a separate path rather than a bolted-on variant.

## The three layers

The split is strict, and it is the reason the feature can be tested
without a window:

| Layer | Crate | Decides |
| --- | --- | --- |
| Layout | `schist-layout` | *What* is where. Pages, composition, styles, threading, the pasteboard plan. Pure. |
| Separation | `schist-separation` | *Which inks*, and writes prepress files. |
| Editing | `crates/editor/src/design/` | *How to draw it* and *what the pointer means*. |

`schist_layout::pasteboard` turns a document plus a view into a
`Pasteboard`: paper boxes, guides with kinds, frames, composed text runs,
shapes, graphics and notes. Nothing in the plan is drawable in itself, and
nothing in it can be edited — it is display data, which is why each frame
carries its `ObjectId` so a click can be answered exactly rather than
guessed from geometry.

`design::paint` takes that plan and paints it. `design::select` takes
points and answers what is under them. Neither computes layout, so a
layout bug is reproducible in a unit test and a paint bug cannot hide one.

`design::tools` sits above both: it turns a press, a drag and a release
into an edit, and it is the only place a gesture is given a meaning.
Everything it changes goes through `schist_layout::authoring`, so a frame a
user draws and a character they type travel on the same undo stack as a
move, and the rules about that are testable without a window.

## One gesture, one undo step

This is the rule the whole authoring layer is built around, and it is worth
stating because breaking it is the failure mode that makes people stop
trusting an editor.

A frame or handle drag records one edit on pointer release. Returning to its
starting point records none. Deleting a selection, page operations, layer
operations and changing multiple objects through Control each use one edit.
A failed batch leaves both the document and history unchanged.

The documented exceptions remain: duplicate, align, distribute and swatch fill
record one edit per affected object; one `set_text` call records one edit.
There is no timed or word-based typing coalescing. One multi-object duplicate
command therefore takes multiple undo steps; it is not claimed to obey the
one-gesture rule.

Creating or duplicating a text frame includes its new story and layer mapping
in the same undo step. Deleting frames retains stories, which may still be
threaded through other frames or edited independently. A locked frame causes
a selection delete to refuse the entire operation.

Each Pen click or handle drag commits one anchor on pointer release. Clicking
the first anchor closes the path in one edit. Enter or switching tools ends
the path; Escape discards a live draft and leaves already committed anchors.

## Two spaces, and one conversion between them

A click arrives in *pasteboard* space: the canvas has been moved and
scaled to fit, and the pasteboard has a margin and a scale of its own. A
document is written in *page* space. `design_page_point` takes the first
back to the second, and the tools then work only in page space.

This is worth stating because getting it wrong fails quietly. At 100% zoom
on a document with no margin, page space and pasteboard space are the same
numbers, every test passes, and the tools are wrong at every other zoom. A
test pins the round trip so the coincidence cannot be mistaken for
correctness.

## The tools

| Key | Tool | Makes |
| --- | --- | --- |
| `v` | Select | Nothing; click to select, drag to move |
| `a` | Direct select | Drag anchors or either Bézier handle |
| `t` | Text frame | A frame, and starts typing in it |
| `m` | Rectangle | A closed rectangle |
| `e` | Ellipse | A closed ellipse |
| `l` | Line | An open path across the drag |
| `p` | Pen | Click anchors; drag handles; click the first anchor to close |
| `g` | Polygon | A five-sided closed path |
| `d` | Delete | Nothing; click to remove |

⌘A selects everything on the page, ⌘D duplicates the selection, ⌘Delete
removes it, and Escape drops the selection or leaves the frame being typed
into. The alignments are ⌘-modified — ⌘L, ⌘C and ⌘R for the left edges,
middles and right edges, ⌘T, ⌘M and ⌘B for the other axis — and ⌘H
distributes the selection, with shift making it go down instead of across.

## Direct selection

`a` puts a shape's own anchor points on the pasteboard, and a click near
one drags it. A click that lands *not* near one selects the whole shape,
because a user who clicks the middle of a small rectangle means the
rectangle and not one of its corners. Both are wanted and only one can be
the default.

A point is stored relative to its frame, and the drag is converted to that
convention in `authoring::set_point` rather than in the editor. A caller
that moved a point to an absolute page position would have to know that
convention, and getting it wrong would displace the point by the frame's
origin every time the frame moved.

The frame follows the curve's tight bounds. Rebasing local coordinates keeps
all unedited anchors and handles fixed in page space. Moving an anchor carries
its handles; moving a handle changes that handle only. All compound contours
are painted and expose their anchors. Ellipses are four editable cubic arcs;
only rendering flattens them, at a tolerance tied to output resolution.

## Alignment and distribution

Alignments work to the *selection's* extent, not the page's, because that
is what a user who has just selected a row of frames means: they share an
edge with each other. Aligning to the page is a different command, and
conflating the two is why so many editors need a modifier nobody can
remember.

Distribution needs three frames to mean anything — with two, the only even
spacing is the spacing they already have — and refuses overlapping ones
rather than pushing them apart, because inventing a layout the user did not
ask for is worse than doing nothing. A locked frame refuses the whole
operation, and is neither moved nor used as the target: aligning everything
else to something the user cannot see or change is how a layout ends up
somewhere nobody chose.

Two things about these are less obvious than they look.

**A click still makes a frame.** Every drawing tool responds to a click as
well as a drag, with a frame of `MIN_FRAME` so it can be selected. A tool
that does nothing on a click reads as broken.

**The line is a diagonal in its own bounding box.** The obvious way to draw
one is a horizontal path in a frame rotated to the drag's angle, and that is
wrong here: the pasteboard positions a path by offsetting its points by the
frame's origin and applying nothing else. A rotated frame with a horizontal
path would draw a level line inside a slanted box, so where the ink is and
where the click lands would disagree. The frame is the bounding box and the
path is the diagonal, which the pasteboard, the hit test and the exporter
all already agree on. It also means a thin diagonal stays easy to click,
because the whole box is live.

## The document panels

Ten panels register through `SidePanel` and `DESIGN_ONLY_PANELS` and restore
from saved dock keys. Each returns `Option<AnyElement>` and hides when it has
nothing to show.

**Pages** leads the column: a layout document is navigated by page, and each
row is a live thumbnail drawn from the same plan the canvas paints, so the
thumbnail cannot disagree with the page.

**Stories** exists because of overflow. Once text threads between frames or
lives on a master page, the pasteboard is no longer a complete view of a
document's text, and a user who cannot find their words has nowhere else to
look. So it lists the document's stories rather than the page's frames, and
says whether each is on a frame or has overflowed. It is the one panel that
can show text nobody can currently see.

**Links** is about the half of a placed graphic that rots. The frame stays
on the page looking exactly as it did, the pixels are gone, and a prepress
check is the first place a user finds out — at the worst possible moment,
which is on the plate. Missing files are counted in the header and marked on
their row. Refresh decodes sources off-thread; Relink preserves placement and
crop in one edit. Decode errors are displayed as unavailable, with their cause.

**Layout Layers** is a compact tree with visibility and lock columns. The
chevron folds a layer; full names are available in tooltips. Drag a layer name
above another layer, or into the slot below the last layer, to reorder it.
Shift-click object rows to select several, then drag a selected name onto a
layer header to move the selection there. Locked objects and locked destination
layers reject the move. Each successful drop is one undo step; a drop which
changes nothing records none. Adding or reordering layers preserves membership
for older documents which omit explicit first-layer assignments.

Pages, Links, Stories and Preflight use icon toolbars with localized tooltips.
Pages retain draggable thumbnails; link filenames and story previews fit one
row, while missing-link causes and preflight findings stay visible. Swatches
show readable names beside colour chips, with their component inputs folded
under Appearance. Character formatting and paragraph alignment use icon buttons
which display the resolved style's active settings.

**Control** edits selection geometry and text-frame columns,
gutter and inset. **Character** and **Paragraph** edit named styles, with
new/rename/apply controls in **Styles**. An input captures its edit target on
focus and commits one edit, so a later selection cannot redirect it.

**Styles** lists the *document's* styles, not ones a user has defined for
themselves. A layout document carries its styles with it, and that is the
point: opening someone else's file gives you the styles their text is
written in, which is the only way that text can look the way they meant.

The two kinds are kept apart because they are applied differently, and
conflating them is a category error rather than a small one. A **paragraph**
style applies to whole frames, so clicking one with three frames selected
restyles three frames. A **character** style applies to a range inside one
frame, so clicking one styles the word around the caret — and falls back to
the word *before* when the caret is past the end, which is where it sits
after typing. Without that fallback the click at the end of a frame would
style nothing and look broken.

Overlapping character ranges are trimmed rather than dropped. Applying
Italic to the middle of a word in Bold text must not strip the Bold from
the rest of the word, so the Bold range survives as two pieces.

**Swatches** is the document's own inks, not a user's saved palette: a spot
colour that is in the swatch list but not in the document is a plate the
prepress stage will not have. Clicking one fills the selection. A spot ink
is marked on its chip, because a user picking an ink is looking at the chip
and not at a count in a header. An ink that is not in the document cannot be
applied at all, which is what stops a fill from producing a document that
cannot be output.

**Preflight** checks the current page on demand using `schist-separation`.
It shows errors before warnings and informational findings, with a page
label and error/warning counts. Coverage is explicitly a 72 dpi preview.
The work runs on the background executor against a document snapshot;
edits and page changes hide stale findings until another check. Opening a
new document discards the old request, so a worker finishing late cannot
overwrite a new result. Checks never edit the document or its undo history.
Checks use the decoded graphic cache, including embedded sources. Unresolved
pixels report errors. Preview CMYK conversion is explicitly not an ICC press
profile and does not certify final output.

The ten are appended to the column only in Design Mode, and a Design preset
saved while designing does not strand them in the photo editor. A panel that
would render nothing is worse than one that is not there.

`Palette` grew a `warning` colour for the missing-link state, chosen not to
look like the selection colour: a warning that reads as a highlight is a
warning that gets filed away.

## What the pasteboard gets right

- **Text is shaped by the shared text engine**, not by the editor's own
  font stack, so the pasteboard shows the same shaping that will be
  rasterised onto a plate. A preview that shaped text any other way would
  disagree with the output, which is the one thing a prepress preview must
  never do.
- **Guides carry a kind** — trim, bleed, margin, column, baseline, ruler.
  Without it a margin guide and a baseline guide are the same value and a
  pasteboard can only draw one colour for all of them, which makes the
  margin guides useless precisely when the baseline grid is on.
- **The trim box is always drawn**, because it is the page's own edge;
  turning the boxes off leaves the edge and nothing else.
- **A hidden page is washed, not removed.** A page that vanishes from
  under the pointer is worse than one you have to notice.
- **An empty frame is still clickable**, because it has no glyphs to click
  and its box is the only way to reach it.
- **A locked object is still hit**, because clicking a locked object and
  having nothing happen is a bug. The hit reports `locked` and the caller
  decides; what it will not do is move.

## Undo

Every edit goes through `schist_layout::History`, so Design Mode's undo
works the way the photo editor's does and a mistake on a page is one
keystroke to fix.

The rule that matters: a drag records **one** edit, at the end, from the
bounds captured when the gesture began. Recording per frame would fill the
undo stack with a hundred entries for one gesture, and undo would then walk
back through the drag a pixel at a time. The starting bounds are recorded
rather than reconstructed from the pointer, because the pointer's last
position says nothing about where the object started — a drag that ends
where it began, or that the 2-point threshold swallowed, would otherwise be
recorded as a move.

## The two pasteboard layouts

InDesign has two, and the original plan called it three — the third is not
a view mode but the pasteboard itself, which is the surface both modes
draw on. Naming it as a third would give a menu entry that does nothing.

The two are not a presentation detail. In spread view a two-page spread is
one object a reader reasons about, and its gutter is meaningful. In
single-page view the facing page is not visible at all, so a spread's
gutter stops being something you can see — which is why the mode is a
document-level choice rather than a zoom, and why switching modes asks for
a refit instead of carrying the zoom across.

The mode is applied to a *copy* of the view. `view.page` is also where the
Pages panel records the current page, and writing it from the mode would
make the two disagree about which page is showing.

Spreads are placed by `LayoutDocument::spread_origins`, laid out left to
right and separated by `SPREAD_GAP`. It is computed rather than read from
`Spread::origin`, because that field is zero in every document built
through the API — trusting it drew every spread exactly on top of the
first, so a document looked like half its length. The field is kept for a
document that stores its own layout, and is documented as not being
consulted when drawing.

## Rulers and the Design viewport

View → Rulers shows rulers measured from the active page's trim. Click the
corner to cycle millimetres, points and inches. Major ticks adapt to zoom,
with four minor ticks between them and fractional labels at close zoom.
Negative values measure the pasteboard outside the page. Rulers are view
state and do not create undo entries; dragging layout guides is still pending.

Scroll pan, wheel/pinch zoom, Fit and Actual Size act on the Design
viewport. The raster document keeps its own pan and zoom. Fit includes the
active spread in spread mode and just the active page in single-page mode,
using both canvas dimensions. Paper, objects, guides, selection outlines,
hit tests and rulers use the same computed spread positions, including on
later spreads whose stored origin hints differ.

## Modes and the dock

`WorkspaceMode` is `Photo` or `Design`. Design Mode is entered by choosing
the Design workspace from the Workspaces menu, which applies both the dock
layout *and* the mode: a layout with a Pages panel in a photo editor is a
blank dock section, which is worse than not offering the mode at all.

The Pages panel leads Design's dock, ahead of the Color panel, because a
page layout document's questions are which pages exist, what is on them,
and which styles the text uses — and it has no image on it yet to say
anything about colour. Each row is drawn from the same pasteboard plan the
canvas paints, so a thumbnail cannot disagree with the page it stands for.

## Painting

`prepare_canvas_paint` returns before any raster work happens in Design
Mode. The tile compositor, the prefetch queue and the browser GPU path all
exist to draw pixels, and a pasteboard has none. The paint closure is one
place for both bodies so they share the canvas element and its event
handlers; a `None` job field means "raster", and a Design frame means
"pasteboard".

Text is blitted as one image per run rather than a path per pixel: a page of
body text is hundreds of thousands of coverage bytes, and a path each would
make scrolling stall.

## Status and what is missing

Working: pages and spreads, a fitted pasteboard, guide kinds, frames with
object ids, text frames composed through the layout engine, hit testing,
rubber-band selection, move-with-undo, the Pages panel, the photo/design
mode switch, and the two pasteboard layouts with their View-menu entries.

Working, additionally: a page layout document **opens from disk and saves
back to it**. The IDML codec reads and writes a real InDesign document in
both directions, is registered behind this same feature flag, and ⌘S and
Save As both reach it. Saving writes to a temporary file and renames, so an
interrupted save cannot truncate what was already there.

Working authoring tools and panels are described below, including text selection,
threading, Story Editor, draggable guides and navigation tools. Output and
layout lifecycle integration are implemented. Frame affines now preserve local
text composition, transformed preview/print geometry, caret hit positions and
native process inks. Independent inner image affines are preserved, including
frame clipping and effective-resolution checks. Advanced IDML text attributes
and other fidelity gaps remain;
self-roundtrip tests are not validation by InDesign. See the [roadmap](roadmap.md)
and [IDML notes](idml-format.md). Production INDD support is still spike-gated.

## Placed graphics

Place decodes a raster source, including PSD, on a background executor and
creates a graphic frame. Import Images as Pages appends one page and background
frame per source in a single reversible operation. Layout stores links, source
dimensions/DPI and original embedded bytes, never a raster `Document`.

The editor owns decoded pixels and preview images. Native fitting supports
Fill, Contain, Original and Stretch, including normalized fitting regions.
A region determines fitting; the frame is the clipping boundary. The image
can have its own affine inside the frame, before the outer object transform.
Preview and output share that inverse mapping, preserving original CMYK values.
File-picker and decoder completions check both the document snapshot and
session identity before applying an edit. Late work cannot modify a newly
opened layout. There is no filesystem watcher; Refresh explicitly rereads
linked sources.

Embedded IDML bytes come from Image/Properties/Contents. They use the same
registered raster codecs as linked files. IDML writes native Rectangle/Image
geometry; Schist fitting metadata uses the public Label/KeyValuePair extension
and is ignored if another editor has changed the native image geometry.

## Localisation

`design.lang` follows the project's convention: English is the source, and
every other locale has a file with exactly the same keys. The original keys are partly
translated; new feature keys use English placeholders under an explicit
`# UNTRANSLATED PLACEHOLDER` marker so the gap is visible rather than
silent.

Long English placeholders require an exact source entry and reason in
[`deferred-english.json`](../crates/i18n/deferred-english.json). The strict
audit prints this debt and permits it only while Design Mode is disabled
by default; source changes or enabling the feature expire the exception.

The i18n suite enforces two things worth knowing before editing these
files: every locale must have exactly the English key set, and every
visible character must be drawable by one of that locale's web fonts. The
second is why the Assamese catalog uses the Assamese block letters
(U+09F0, U+09F1, U+09C8) rather than the Bengali-block forms — the bundled
font has no glyph for the Bengali ones, and the substitution is also the
correct orthography.


## Story editing and navigation (Phase 3 items 7–8)

The Stories panel opens a separate Story Editor window. It uses the same
layout document and undo stack, observes external edits, and rejects writes
if the originating document session has been replaced. It lists all stories,
including unplaced text, with frame In/Out ranges and overset status. Canvas
text clicks and drags use the shared shaping engine for caret hit tests and
selection rectangles. Selection replacement and clipboard insertion are one
edit per call. IME composition is a draft until committed, then one edit.

Unsupported imported tables, footnotes and inline page items retain their raw XML
and source anchors. Story Editor shows a compact count, and preflight treats their
missing appearance as an error. They do not contribute body characters or paint.
Typing at an anchor inserts before it; a replacement crossing an anchor is refused.
Text and style edits preserve the data through one undo step. Opaque-only stories
are occupied thread targets, even when they have no ordinary text.

IDML saves retain this data in a guarded Schist Label, not as working native tables
or footnotes. Reopening an unchanged save preserves its anchors. External changes
to the native story take precedence; retained payloads then have unknown locations
and remain diagnosed. Referenced native resources and full structured composition
are not reconstructed by this preservation step.

Document footnote preferences now retain numbering, restarts, affixes, paragraph
and marker styles, spacing, baseline policy, splitting, column spanning and both
separator rules through native IDML Preferences parts. Unknown references stay
explicit and are reported. Text-only note bodies now have their own typed stories,
styles and zero-width marker coordinates, separate from main-story characters.
The original XML remains exact; notes with nested objects, tables or unknown
instructions stay opaque. Style renames update typed note references in the same
undo step. Bodies and markers still require composition and reserved space before
they can print.

Text threads have explicit order independent of page and layer order. Click
an output port, then an empty frame to link; the Stories panel also offers
link and detach controls. Detaching leaves the original text in its original
story. Linking refuses to overwrite another nonempty story. Locked threads
refuse edits. Native IDML PreviousTextFrame/NextTextFrame references preserve
flow across spreads. Parent-page threads have kernel and repeated-save coverage;
their native application behavior still needs validation.

Hand (H), Zoom (Z; Alt to zoom out, double click to fit), and Eyedropper (I)
are Design tools. Space or the middle button temporarily pans. Eyedropper
copies a shape's fill, stroke, opacity and overprint to selected shapes in
one edit; it does not yet sample image pixels. Shift-click toggles objects
in the selection; drag empty space to select with a band. Moving a selection
preserves its relative geometry and records one undo step for the whole drag.

Drag from a ruler to create a page guide. Drag an existing unlocked guide to
move it, or back onto a ruler to remove it. Release records one edit; Escape
cancels. Guides and Snap toolbar toggles control visibility and snapping.
Guides are page-local model data and are written as native IDML Guide elements.
The Guide encoding has synthetic round-trip tests, not an independent vendor
fixture. Shape creation now displays its live outline, and the Line tool
preserves both endpoints for horizontal, vertical and negative-direction drags.


## Output and document lifecycle (Phase 3 item 9)

Design File → Output opens resolution, sequential 1/2/4-up, printer-mark,
hidden-page and CMYK-profile choices. PDF contains rasterized process/spot
plates and an optional embedded CMYK output profile; it does not claim PDF/X
conformance or editable vector/text output. Independent Poppler checks cover
all four proof inks, marks, page sizes, n-up reading order and incomplete sheets.

Native CMYK artwork keeps its original composite channels; RGB previews are
used for display. RGB artwork and paints can use the selected ICC transform.
Authored CMYK paints keep their explicit builds. Preflight blocks output on
missing artwork, missing fonts or overset text; low effective image resolution
and ink-coverage warnings are also surfaced. Checks run on immutable snapshots.

Package produces a ZIP with layout.idml, the original linked artwork under
collision-safe relative names, and a JSON manifest listing assets, fonts and
warnings. It never copies font files. Package and PDF output use unique sibling
temporary files followed by atomic replacement; failed writes leave no orphan
temporary file and preserve the previous output.

New, Open, Close, Quit and the native window close check saved layout content
independently from raster tabs. Undoing back to the saved state clears dirty
status. Save/Discard/Cancel apply to the pending transition. Delayed loaders
and save dialogs check document/session identity, and relative external links
are resolved against the opened layout before a later Save As changes folders.

The Pages panel edits trim size and four margins on the current page. Four-sided
bleed and slug offsets apply to all pages in one undo step, with inside/outside
following the spread spine on facing pages. Slug is measured from trim,
independently of bleed. PDF media encloses both and reserves room for marks.
Numbering controls edit the current section: start/remove a boundary, restart or
continue numbering, choose any of five styles, edit the prefix and its visibility,
and retain a section name and marker. Fields capture their section when focused;
each committed value is one undo step. Geometry values use points; text
tracking uses thousandths of an em. Paragraph font families are editable.
Local supported IDML formatting becomes editable named styles with a notice;
paragraph font/paint inheritance and native paragraph/structural breaks now
survive saves. Blank paragraphs reserve line space, and balanced paragraphs
use distinct vertical positions. The browser check passes using the native
build's cached backer catalog. Native window visual validation is blocked:
Computer Use was not approved for Schist. The debug application builds.

## Ink tint controls

Control edits fill and stroke tint percentages for selected shapes. One committed
field changes the captured selection in one undo step; invalid values and locked
selections leave the document unchanged. The eyedropper copies both tints with the
paint. Character and Paragraph panels edit named styles' fill/stroke tint; clearing a
style field restores inheritance. All controls use 0–100%, independently of
opacity. Zero tint is paper that still knocks out underlying ink.

The model stores fractions separately from full-strength ink definitions. This
keeps every tint of a spot on the same plate. Canvas shapes and text paints show
these values, and separation applies tint after ink aliases and process builds.
Text strokes and inherited object-style paint are rendered and editable, as
described below. Native window visual QA remains outstanding.


## Text decorations

Character and Paragraph edit underline and strikethrough independently. Each
enabled flag, line paint, point weight/offset, tint and overprint setting inherits
separately. Blank dimensions inherit; Auto resets an ancestor's explicit dimension.
Text color resets an inherited line ink to the current glyph fill; no ink suppresses
the line without discarding its other settings. A custom ink can remain visible
when glyph fill is disabled. Each captured style change is one undo step.

Preview and print share the renderer in horizontal and both vertical directions.
Automatic dimensions retain font/column defaults. Explicit horizontal offsets
place line centers below the baseline for underline and above it for strike;
vertical offsets measure from the column center, positive toward the outside
(right in right-to-left columns, left in left-to-right columns). Dimensions scale
with zoom and output DPI, without changing shaping, wrapping or carets. Fractional
solid weights use exact pixel area. Stripes, unadjusted dashes and gap
paints are described below. Dotted and path decorations are described below; native
application agreement still needs visual validation.


Paragraph and Character controls now expose baseline offsets in points. Blank
restores inheritance; zero explicitly resets an inherited offset; positive values
raise horizontal text or move vertical text right. Each field commit is one undo
step against the style captured on focus. Glyphs, underlines, strikes, carets and
selections move together, and hit testing follows their visible positions even
with frame transforms. Line spacing, flow, baseline grids and drop-cap reservations
stay fixed. Automatic script sizing and positioning use the controls below.


The Swatches panel now edits base RGB or native CMYK components, creates named
tints and edits their percentages. A base edit updates matching uses in shapes,
styles, parent artwork and its named tints in one undo step. Same-named unrelated
inline colors are preserved. Tint edits update all uses together; generated names
follow the percentage while imported custom names remain intact. Applying a
swatch retains the documented per-object fill undo behavior. Direct tint controls
detach a named tint to its base color before assigning the new percentage.


Paragraph and Character now select inherited, normal, superscript or subscript
position independently of the explicit point offset. Character also edits the
four document-wide script size/position preferences. Each committed choice or
field is one reversible style-context edit. Empty numeric preference fields and
out-of-range values are rejected; position inheritance has its own explicit choice.
Preferences change all affected stories, with defaults and the supported range
matching the native XML specification. Automatic OpenType glyph variants remain
unsupported and import reports them.


Paragraph and Character expose OpenType overrides as comma-separated four-byte
tags and 0/1 values, for example `liga=1, dlig=0`. Blank restores inheritance;
omitted tags inherit independently, and zero explicitly disables a feature.
Committing the field edits its captured named style in one undo step. Invalid
syntax or duplicate tags leave the style unchanged. Features flow through the
shared shaper into measuring, wrapping, carets, preview and print; equivalent
feature settings on adjacent ranges do not split a ligature. Font support still
determines whether a requested substitution exists.

Native IDML boolean switches, figure styles and complete stylistic-set masks are
retained. Arbitrary tags and partial atomic groups use a standard Label and an
export notice because another application cannot reproduce their inheritance
from the corresponding native attribute. Paragraph and Character also expose independently inherited mode-dependent CJK
kana and proportional metrics. Enabled selects `hkna`/`palt` horizontally or
`vkna`/`vpal` vertically; disabled clears both axes. A nearer mode switch clears
inherited tags in its pair, while equally near explicit tags take precedence.
Choosing a switch in the UI clears its same-level tag exceptions in one undo step.
Native IDML retains these switches; independent tag exceptions use the reported
feature label, guarded so later native edits take precedence.


### Automatic and fixed leading

Character and Paragraph accept a point value, `Auto`, or a blank field for
inheritance. Zero is explicit overlapping leading. Paragraph also exposes the
inherited automatic percentage, from 0 to 500; unresolved Auto defaults to 120%.
Each field edit targets the captured style and undoes once.

Fixed leading sets baseline spacing independently of nominal font size. Auto
uses each run's nominal size, including superscript/subscript, and the largest
request on the incoming line controls the spacing. Empty paragraphs use the
paragraph request. The first line fits by its font metrics, without reserving a
full leading interval before it. Vertical columns use center spacing. Grids may
increase spacing. Nominal cells and caret segments remain measurable even when
leading is zero; intentionally overlapping cells are retained.


### Named font variants

Character and Paragraph expose an exact font-style name, such as Light or Bold
Condensed. A blank restores face inheritance. Choosing bold or italic resets the
named face and uses the conventional weight/slant request. The field targets the
style captured on focus, and each commit is one undo step. Family and type size
inherit independently. All Design property buffers now support ordinary text
editing and clipboard operations; numeric fields still validate their values on
commit, while Auto leading, feature syntax and section text accept letters.

Preview and print resolve the requested static face from the font's typographic
subfamily. Missing variants receive a preflight error even when the family is
installed. Font installation in a session invalidates fallback caches. Variable
font axes and named variable instances remain unsupported. Native PSD/Affinity
writers retain the existing private/pixel or reported raster fallback for the
shared renderer's new named-face setting.


Object paint styles are editable in Styles. New style captures the first selected
frame's visible paint. The definition has a name, an optional base, independent
fill/stroke category switches, colour, tint, stroke width and overprint. Blank
numeric/base fields restore inheritance; no paint explicitly clears an ink.
Apply clears local overrides in enabled categories while preserving disabled
categories. Detach keeps the current paint. Apply, detach, rename and definition
edits each undo once for any selection size. Locked objects are skipped.

Control now edits fill/stroke swatches, percentages and stroke width for shapes,
text frames and image frames. An unchanged numeric value leaves named tint
inheritance intact. Eyedropper transfers resolved frame paint and opacity; Swatches
updates object styles and local/parent frame paint. The existing per-object undo
exception for filling from Swatches remains deliberate.

Frame fill draws before content, with stroke afterwards. Frame outlines scale
with their boxes and share frame affines, separation tints and overprint rules.
Curved text-frame outlines are retained for paint, but composition still uses the
rectangular box; import and export disclose that difference. Synthetic frame
paint does not expose editable shape anchors. Object-style effects, paragraph
application, corner/stroke patterns, text wrap and fitting categories are not
implemented by these paint controls; unsupported native categories are reported.

Text paints are now editable in Character and Paragraph. Fill and stroke each
allow inherited, no-ink or named swatch values; a no-ink choice explicitly clears
an ancestor's paint. Stroke weight is an absolute point value, with centered or
outside alignment. Tint and overprint are independent for the two paints. Each
control change targets the captured style and undoes once; blank numeric values
restore inheritance. The renderer strokes actual font contours, including curved
glyphs and vertical/rotated text, without changing line advances or carets.
Preview composites the separate paint coverages, preserving translucent overlaps.
Stroke joins independently inherit miter, round or bevel geometry. Character and
Paragraph also edit the nonnegative miter limit; blank restores inheritance and
zero bevels every nonstraight miter. The ratio stays unchanged with output DPI,
while stroke point widths scale. Join edits leave line metrics and carets intact,
and each captured style change undoes once. Striped decorations are described
below; supported dashes follow them. Dotted and path decorations are described below.


### Capitalization

Character and Paragraph offer inherited, normal, all caps, small caps and OpenType
all-small-caps choices. A choice sets both legacy capitalization flags in one
captured, undoable edit; older independently inherited flags remain supported and
are labelled Partial inheritance. Character's document preferences include the
synthetic small-cap percentage, from 1 to 200 (default 70). Nine new keys are in
all 150 catalogs.

All caps changes displayed glyphs without changing the story. Unicode expansions
such as `ß` to `SS` keep original byte clusters, selection and caret boundaries.
Small caps uses real substitutions where available, otherwise scaled uppercase
graphemes; combining marks stay with their base. Nominal line metrics remain
unchanged, including automatic underline/strike geometry. Oversized synthetic
caps contribute ink across gutters beyond their nominal frames. OpenType all-small-caps requests native `smcp` and `c2sc` without
synthesis. Explicit feature tags override those defaults, including disabling
small caps. Casing uses Unicode uppercase mappings with Turkic and Lithuanian
tailoring selected by the resolved language (see below). Native-application visual
agreement remains unverified.

Native IDML Capitalization and SmallCap preferences survive repeated saves. A
legacy partial flag pair requires reported Schist metadata because the native
property is atomic; a later native capitalization edit takes precedence. Existing
public fixtures now retain their previously dropped AllCaps local ranges.


### Striped underlines and strikethroughs

Character and Paragraph expose line pattern, stripe edges, gap color, gap tint
and gap overprint. A stripe definition is a sequence of increasing start/end
percentages of the line weight, for example `0 25 75 100` makes two outer bands.
The pattern picker includes imported named definitions. Solid explicitly resets
an inherited pattern; clearing a stripe edge value restores inheritance. Unchanged
fields preserve imported names and explicit Solid resets. Each control uses its
captured style target and commits one undoable edit. Seven labels are present
in all 150 catalogs.

Gap paint is independent of line paint. It may inherit, use text color, select a
swatch or explicitly use no ink. Named Tint changes follow the existing base-color
detachment rule. Gaps can remain visible when the main line or glyph fill has no
ink. Equal stripe and gap inks become one solid silhouette before opacity is
applied. Point weights and offsets scale with zoom; stripe percentages do not.
Capped dashes are implemented below. Dotted patterns and decorations following text paths are described below.


### Dashed underlines and strikethroughs

The pattern picker also includes unadjusted dashes. Dash/gap lengths
are alternating point values, at most five pairs; they scale with canvas zoom and
print resolution. Zero-length members are allowed when the whole cycle is
positive. The dash phase continues across characters, spaces, bidi runs and
paint-only changes along each uninterrupted line. An offset, width or pattern
change begins a new pattern. Main and gap inks retain the same independent
color, tint and overprint controls as stripes.

Unchanged dash fields preserve imported resource names and do not add undo steps;
each real edit commits once. Two labels are present in all 150 catalogs. Native
IDML retains named resources, opaque references, inherited lengths and local
formatting across repeated saves. Straight endpoint fitting is described below; native application visual
agreement remains unverified.


Dash definitions retain butt, round and projecting caps. Character and Paragraph
expose the cap choice for an explicit dashed pattern. Editing lengths preserves
the imported name and cap; each cap change targets the captured style and undoes
once. Four labels are present in all 150 catalogs. Earlier array-only serialized
dash definitions still load as butt-ended patterns.

Caps extend at real dash endpoints. Internal character and paint boundaries do
not create extra endpoints. Round zero-length dashes are circles; projecting
zero-length dashes are squares. Overlapping caps form a single paint silhouette,
so opacity applies once. Line layout and caret positions remain unchanged.

Automatic capped lines reserve their font-derived thickness in cross-gutter
contribution bounds, even when the frame itself does not touch the other page.
The output property checks automatic and explicit weights on both spread sides.


### Dotted underlines and strikethroughs

Character and Paragraph offer a dotted pattern and a dot-center spacing field.
One to five point intervals repeat along the line, with a positive total cycle;
line weight sets circle diameter without moving centers. Named imported resources
and explicit Solid overrides survive unchanged field commits. Real pattern or
spacing edits use the captured target and undo once. Two labels are in all 150
catalogs. Native IDML resources, inherited settings and local formatting survive
repeated saves.

Dot phase continues across characters, spaces and paint changes. Overlapping dots
form one silhouette before opacity, and gap inks remain independent. Automatic
and explicit dot thickness contributes across both sides of page gutters.
Straight endpoint fitting and decorations following text paths are described below.


### Fitting straight decorations

Character and Paragraph can fit dashed patterns to the ends of each continuous
line by adjusting dashes, gaps or both. Dot fitting adjusts center intervals
without changing diameter; gap-only and combined settings are available. The
choice belongs to the named stroke resource. Editing its lengths preserves both
name and fitting; an unchanged commit adds no undo step, and each real edit
undoes once. Five labels are present in all 150 catalogs.

The shared renderer chooses complete dash sequences that minimize proportional
change, while keeping the other component fixed. Gap fitting on a line too short
for two complete fixed-length dashes closes the gaps and clips the final dash.
Dash-only fitting can grow a zero first dash when proportional scaling cannot
fit it. Zero intervals otherwise remain zero. Character/paint boundaries do not
restart fitting. Fitted endpoints are explicit so division rounding cannot erase
a terminal dot or cap.

These are Schist's straight-segment rules. The public specification identifies
which lengths may change but does not define numerical repetition selection.
Native application agreement and fitting around path corners remain unverified.
Dot resources requesting dash-only adjustment remain diagnosed as unsupported.


### Text languages

Character and Paragraph accept language tags, such as `tr`, `ro` and `en-US`.
Blank restores inheritance; `und` explicitly requests default shaping and casing.
Committing a field edits the captured style once. Leaving an imported language
unchanged retains its native resource identity and dictionary settings. Authored
tags have a distinct representation, so an imported resource ID that happens to
look like a tag cannot intercept an edit.

Paragraph defaults, character inheritance and local ranges now carry language to
OpenType shaping, including ordinary Latin text using automatic direction. Font
language systems can select localized glyphs. All caps and synthesized small caps
use Turkic dotted-I rules and Lithuanian dot removal from Unicode 17 SpecialCasing.
Casing reads the original source context, keeps story bytes and grapheme carets,
and supports horizontal and both vertical writing modes. Equivalent empty/`und`
resets do not introduce shaping boundaries. Other locale-specific CLDR tailorings,
hyphenation dictionaries and spell checking are not implemented by this change.

Native IDML language declarations retain opaque IDs, names, quote pairs, dictionary
vendors, numeric identifiers and labels. Known authored tags lower to observed
native language names; guarded metadata keeps the exact authored tag. Unsupported
native mappings use No Language with an explicit export notice and preserve the tag
as metadata. Later native reference or resource changes take precedence. No claim
of external InDesign rendering agreement follows from a Schist round trip.

## Text on a path

Select a single-contour shape and choose **Text on path** in Control. Conversion
keeps its identity, layer, affine and frame paint, creates one story and starts
text editing. One undo restores the original shape and removes the new story.
The existing Direct Selection tool edits baseline anchors and handles; the
original story and bracket distances survive these edits. Duplicate copies the
story independently, as with rectangular text frames.

Control exposes start/end arc distances in points. A blank end follows the curve;
an explicit end remains an absolute distance when the curve changes. Shortening
past an explicit bracket leaves the container overset until its brackets are
adjusted. Captured multi-object edits validate the whole selection and undo once.
Unchanged displayed values retain imported precision; an explicit commit still
normalizes mixed raw values that round to the same display. Box-only columns, gutters
and insets are hidden for path containers.

A path accepts one shaped line and threads excess text to another path or box.
Paragraph and structural breaks advance through the same story coordinate system;
UTF-8 source text and grapheme carets remain unchanged. Paragraph indents and
alignment operate inside the brackets. Grids, multiline keep rules, enlarged
initials and vertical story orientation do not add rows to a path. Visible glyphs
and insertion segments remain clickable above/beside a zero-height baseline,
including under zoom and object affines. Rotated glyph bounds also contribute to
neighboring-page output when only the text crosses the gutter.

Native IDML uses a TextPath child on a Polygon parent, with the child's identity
in mixed path/box threads. The supported effect is horizontal text following the
baseline tangent, with baseline/center-of-stroke alignment. Other native path
options are diagnosed. Underlines and strikes follow the path, including solid,
striped, dashed and dotted paints with independent gap inks. Pattern phase and
endpoint fitting span consecutive characters before bending; a formatting
boundary does not restart them. Sharp joins use Schist's bounded miter/bevel
raster policy. Native corner-fitting agreement and additional effects remain
open. The integration proof compares independently constructed shared-engine
baseline specifications; native GUI and external application agreement remain
unverified. Design Mode remains disabled by default.

### Automatic list markers

Paragraph styles now carry independent list kind, bullet, numbering start and
continuation, expression, marker formatting and tab settings. The Paragraph
panel exposes list kind, a Unicode bullet, starting number, number format, expression,
continuation and list tabs. Blank fields inherit; `^#` inserts the current number
and a final `^t` separates the marker from the body with a tab. Existing left and
first-line indents position the marker. Field targets are captured on focus.

Generated markers are separate paint records with zero source length. They do
not enter the Story Editor, clipboard, selection or caret navigation. Numbering
follows paragraph order and named sequence identity, independently of frame
wrapping; explicit restarts take precedence. Markers inherit the first character
before their own character style is applied. The initial support is horizontal
Unicode bullets and decimal, Roman, alphabetic and padded sequences at levels 1–9
within a story or across known same-page unthreaded frames. The list disclosure
selects a numbering sequence and continuation across stories; this setting belongs
to the shared resource, so every referencing style observes the change. Creation
order is independent of stacking and survives undo and IDML saves. Unknown imported
chronology and threaded, parent, multiple-page or book sequences are diagnosed.
Preflight warns that supported cross-story ordering uses Schist chronology; native
rendering equivalence is unverified. A hidden-number format retains any expression literals and tabs.
Unchanged format choices retain imported native names and types. Roman values
outside 1–3999 are preserved and diagnosed. Unsupported
native list options are preserved and diagnosed by IDML import/export. Empty items reserve their marker and caret, including overset when a frame is
too narrow. Marker ink outside a frame remains clickable without becoming an
editable caret stop. Editor, output, native/browser/headless checks and the full
sweep pass. The previously sandbox-denied clipboard HTTP test also passes in
the unrestricted publication rerun. Native application rendering remains unverified.


Multilevel numbering adds a level field and higher-level restart selector.
Levels 1–9 may include previous levels with `^1` through `^8`; `^#` is the current
level. Restart choices inherit, restart after any higher level, or continue the
current level. Disabling a retained native policy keeps its definition; choosing
inheritance clears the local policy override. All fields use captured targets
and one undo step. The multilevel full sweep passes all eligible tests (1,654 distinct Rust tests
plus four browser tests); its unchanged clipboard listener was sandbox-blocked
at that checkpoint and passes in the later publication rerun.


### Paragraph tabs

Paragraph tabs accepts point positions for ordinary paragraphs as well as lists.
Unmodified Tab inserts a source tab in the canvas editor and Story Editor. A
captured style edit or text replacement remains one undo step. Blank tab fields
restore inheritance; existing imported alignment/leader records are preserved
when their displayed positions are committed unchanged.

Leading stops are measured from the column origin, independently of first-line
indents, list markers and enlarged initials. Wrapping, painting and caret placement
share that origin in horizontal and vertical flow. A leading tab which cannot fit
remains overset and can resume in a wider threaded frame. A terminal tab after
text can end at the frame edge, letting its following field wrap without losing
the source tab. An ahead-of-pen aligned stop clamps a colliding field to the pen;
only passed stops are skipped. This collision rule follows a public native PDF,
documented in `docs/idml-format.md`. Justification expands ordinary
spaces after the final tab; earlier fields retain their stop positions.

Left, right, centered and character/decimal stops compose using the following
field's shaped metrics. The selected stop can have a literal Leader pattern of up
to eight characters, including spaces. Complete shaped units fill the gap against
the following field's edge and inherit the source tab character's formatting.
Leaders add paint without changing source text, wrapping or caret positions.
Their exact native repetition phase remains unverified.

An ahead-of-pen hanging indent supplies a virtual leading stop before a later
explicit stop or Schist's 36-point grid. It carries no leader; an explicit stop
at the same position retains its own alignment and leader. The public native
reference establishes horizontal LTR indent placement. The same logical geometry
is used for other directions and axes without claiming native agreement there.
Missing/empty stops report this indent/grid fallback in IDML. Horizontal
RTL tabs use a right-edge ruler with right paragraph alignment; vertical RTL tabs
keep downward flow. Other centered/right paragraph-alignment cases, justified
non-leading stops, vertical path tabs and tabs inside an enlarged initial remain diagnosed. Those initials use ordinary
source flow rather than scaling an unsupported tab gap. Their native
records are retained. Native application rendering agreement is not established.


Design preview uploads use GPUI's BGRA byte order for text, fills and artwork.
Decoded RGBA and native CMYK remain the print/transform sources. The Layers tree
uses text excerpts or translated object kinds for unnamed native objects; these
are display labels and do not rename the IDML objects.
