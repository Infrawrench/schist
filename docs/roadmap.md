# Roadmap

Where Design Mode actually is, what gates what, and what is deliberately
not started. Written because phase status otherwise lives only in a
conversation, and the gating below is the part that is easy to get wrong:
**Phase 0 gates Phase 5**. The kernel and IDML authoring are implemented;
integration still has composition and interchange fidelity gaps to close.

## The one-line version

The layout engine, IDML open/save and main authoring UI work. Design Mode ships
dark behind `design-mode` while Phase 3 integration and output fidelity are completed.

## Status

| Phase | Scope | State |
| --- | --- | --- |
| 0 | INDD spike: fixtures, container map, go/no-go, `docs/indd-format.md` | **Started — eleven public pairs acquired; database semantics unresolved** |
| 1 | `schist-layout` kernel: model, geometry, styles, stories, composition, threading, grids, undo | **Implemented — composition integration fixes ongoing** |
| 2 | `schist-codec-idml`: reader, then writer, lossless round-trip | **Done** — both directions verified, and wired to open and save |
| 3 | Design UI: pasteboard, panels, tools, story editor, rulers | **In progress — items 1–8 implemented; item 9 output integration in progress** |
| 4 | Output: prepress PDF, preflight, IDML/INDD export, CCF package | **PDF export, profile choice, n-up and package UI implemented; fidelity/validation ongoing** |
| 5 | `schist-codec-indd`: reader, then writer if the spike justifies it | **Not started, spike-gated** |

## Done

**Phase 1 kernel.** `schist-layout` has pages and spreads, parent pages
with override tracking, layers, frames, links, an ink model, named
paragraph and character styles, grids and snapping, per-paragraph
direction and writing mode, and operation-based undo. Composition handles
justification, column balancing, paragraph spacing, independently inherited
paragraph keep policies, indents and forced breaks. Integration review found that drop-cap geometry never reached glyph rendering
and grid leading alone did not align baselines to page guides. Horizontal and vertical
initials now paint their enlarged glyphs once, and measured baselines snap to
the correct page grid. Vertical composition, native story orientation and paragraph direction now
have integration coverage. Advanced typography and interchange still have
explicit gaps before the kernel can be called complete. Current test counts and
checks are in Handoff.

**The pasteboard plan** (`schist_layout::pasteboard`). A document plus a
view becomes paper boxes, typed guides, frames carrying `ObjectId`s,
composed text runs, shapes, graphics and notes. Pure, so the whole visual
layer is testable without a window. See [Design Mode](design-mode.md).

**Part of Phase 3**: the mode switch and its dock preset, the
`design-mode` flag, the pasteboard painter, hit testing, rubber-band
selection, move-with-undo, the Pages panel, the two pasteboard layouts
with their View-menu entries, and the first authoring tools: select, text
frame, rectangle, ellipse, line and polygon, with typing, a caret, direct
selection of anchor points, align, distribute, duplicate, delete and fill
from a swatch, paragraph and character styles; and the Stories, Links,
Swatches, Styles and Preflight panels.

The tools are the point of the phase. Before them Design Mode could open
a document, show it and move what was already on the page, which is a
viewer with a drag. What is still missing is the rest of a layout editor:
integrated output and the remaining interchange fidelity fixes. Layers,
Control, Character, Paragraph, Place, image-page import, relinking, Pen,
Story Editor and text threading are implemented. Navigation tools and guide
gestures are implemented.

**Part of Phase 2**: the IDML OPC/UCF package and part index
(`schist-codec-idml`, 28 tests), with the container cross-checked against
the system `zip` and `unzip`. `docs/idml-format.md` records which parts
are verified and which are still only read from the specification.

**Part of Phase 4**: the prepress PDF writer in `schist-separation` — real
page geometry with bleed and crop marks, TrimBox/BleedBox, imposition,
a DeviceN/NChannel image preserving all plates, tint transforms and
registration colorants — plus halftoning, trapping, separations preview and preflight.
`make check-separation` is green.

**The flagged CMYK item**, which the plan deliberately left out of scope
and which turned out to be a project in its own right: spot inks,
overprint, knockout, UCR/black generation, editable separations and an
overprint display simulation, with PSD/PSB interchange.

## The gating, stated plainly

```
Phase 1 (integration fixes) ──► Phase 2 (IDML subset) ──► Phase 3 ──► Phase 4 export

Phase 0 (INDD spike, eleven paired samples) ──go/no-go──► Phase 5 (INDD)
```

- **Phase 2 is complete for the supported IDML subset.** Layout documents
  open and save. Remaining integration and fidelity work keeps the feature dark.
- **Phase 0 gates Phase 5 only.** IDML does not depend on the INDD spike,
  and no INDD work should start before the spike's go/no-go. If the spike
  fails, INDD stays a reader-only link-extraction feature and IDML remains
  the write target, which is why both codecs target the same
  `LayoutDocument`.
- **Phase 4 splits.** PDF kernel code exists, but output validation found
  invalid PDF colour-space declarations and plate images that erased prior
  inks. The corrected writer now renders all four proof patches in Poppler.
  Output integration is implemented in item 9, including sequential n-up and
  CMYK ICC selection. Poppler pixel checks cover the proof inks and n-up
  sheets; string-based tests alone were insufficient.

## Not started, and what each one needs

### Phase 0 — INDD spike (1–2 weeks)

Needs 6–10 real `.indd` fixtures (v18–v21 plus older) with a matching
`.idml` for each, so there is always a spec-readable reference for the same
document. Then map the container: master pages, the object-stream walk,
decompression, and whether the stream is indexed. Commit findings as
`docs/indd-format.md`, and keep the AI session transcripts with it per
`AGENTS.md`.

**No Adobe header files and no decompilation.** Public sources and
observable behaviour of our own fixtures only, exactly as
`docs/affinity-format.md` was done.

Publicly distributed INDD/IDML pairs may be acquired under the user’s
2026-09-29 authorization. Eleven acquired pairs now cover an older version,
v19.5 and v20.2, with three redistributable pairs recorded in
`fixtures/indd/README.md`. Version 18/21, controlled changes and database
semantics are still required before a format go/no-go.

### Phase 2 — IDML (2–3 weeks)

Reader first, then writer, one file per IDML part, `mimetype` first and
stored per UCF. The acceptance test is that Schist → IDML → Schist is
lossless for the supported subset, with `make check-idml` mirroring
`check-layered-codecs`.

**Done:** the OPC/UCF container and the part index
(`schist-codec-idml`, 28 tests). `docs/idml-format.md` records why the
container is hand-written — the `zip` crate's available version is yanked
and the crate has to build offline — and how the test suite compensates by
using the system `zip` and `unzip` as the other side of every comparison.

**Specimens found, and they changed the code.** Seven real InDesign
exports are in `fixtures/idml/`, from a vendor that publishes them for
testing its own importer — plain XML inside a ZIP from a published
specification, so no Adobe binary was read. They are covered by
`fixtures/idml/README.md`.

They were worth more than expected: the container and part index were both
already written and both were **wrong**. Object ids do not carry their type
(`u39c`, not `Story_u39c`), most objects are inline and have no part of
their own, and two of the seven documents have no story at all. Each is
wrong in the same direction — about what a conforming document always
contains — so a reader built on any of them would have opened most files
and failed on the rest. `docs/idml-format.md` records all three, with the
object encoding (geometry, stories, frames, inks, links) as read from the
real files rather than guessed.

**The reader is done for the subset the specimens cover.** `import.rs` turns
a real export into a `LayoutDocument` that the layout kernel composes and
the pasteboard draws, and every file reports what it could not read rather
than dropping it quietly. Getting there found a bug in another crate:
`Rect::union` in `schist-layout` treated a zero-size rectangle as absent,
so `ShapePath::bounds` placed every shape at its last point. Fixed, with a
test.

**The writer is done for the same subset**, and held against the reader in
both directions: a document written and read back is unchanged, and a real
export rewritten and read again is unchanged. Writing found three more
faults, one of them in the reader: a run carrying IDML's
`[No character style]` was becoming a style range, so most files filled
the range list with entries named after the marker. Also that text outside
every character range was being dropped, which deleted most of any
paragraph that had styling on it.

**And it is wired both ways.** `LayoutCodecPlugin` is a parallel trait in
`crates/plugin-api` — `CodecPlugin` is typed on the raster document, and
widening it is how a layout engine ends up inside the image editor's data
model. It has its own registry list and its own lookup, the IDML codec
implements it behind the `design-mode` flag, and `load_file` routes a
layout file into the design state and switches to Design Mode. The status
line says what could not be read.

That matters more than it sounds: an IDML handed to the *raster* decoder
does not error, it produces a small grey image the size of a page. So a
routing mistake looks like a successful open. `tests/open_path.rs` asserts
the two lookups never cross.

**Saving goes through the same trait.** ⌘S and Save As both reach it, the
write is to a temporary file and a rename so an interrupted save cannot
truncate what was already there, and Save As suggests `.idml` rather than
a raster format a layout document cannot be written as.
`tests/save_path.rs` goes through a real file rather than memory, because
that is where a save actually fails: a missing directory, a second save
over a first, a temporary file left behind, and a save that drops content
without saying so.

With the codec done, what is left for Design Mode is the rest of Phase 3 —
and that, not the format, is why the feature is still dark.

**Specimen gaps, known rather than assumed.** Public Penn State and OAC templates
now cover facing spreads, populated masters, spot resources and Japanese text.
Real overridden master items, overprint, RTL/foldouts and external application
rendering remain validation gaps. Frame affines now retain oriented text, graphics and shape strokes; independent inner image
rotation, reflection and shear are also retained with frame clipping.
Supported local text formatting is lowered to reusable named styles with a
notice. Paragraph font/paint defaults, opaque style IDs and paragraph versus
soft/structural breaks are preserved. Advanced text attributes remain partial.
Named styles, inheritance and resource fonts are read and written.

Also, and independently of specimens:

- **`CodecPlugin` cannot carry this codec**, and this is the one place
  from the original plan that had to change. `import` and `export` are
  typed on `schist_core::Document`, the raster document — the same wall
  `ToolPlugin` hits. There is now a parallel `LayoutCodecPlugin` trait in
  `crates/plugin-api` with its own registry list, rather than a widened
  `CodecPlugin`.

### Phase 3 — the rest of the UI

This is now the work between Design Mode and the planned layout editor.
Documents can be opened, authored with text and shape frames, and saved;
items 1–8 below are implemented, with integration and output work continuing in item 9.

Layers, Control, Character and Paragraph panels now exist, alongside Pages,
Stories, Links, Swatches, Styles and Preflight. Page add/remove/reorder,
visibility, parent assignment and layer operations have reversible model
operations. Place and PSD/image-per-page import decode through raster codecs;
relinking preserves graphic placement. The remaining ordered work is listed
in Handoff; output integration follows the current tool verification.

**A constraint that invalidates part of the original plan:** the design
tools were to live in `plugins/tools-design` as `ToolPlugin`s. `ToolPlugin`
is hard-bound to `schist_core::Document`, so Design tools need a parallel
trait. The interaction path built so far lives in
`crates/editor/src/design/` instead, which is the smaller change; the
question of whether to also expose it as a plugin trait is still open.

Pages, Stories, Links, Swatches, Styles, Preflight, Layout Layers, Control,
Character and Paragraph are registered in
`SidePanel` and `DESIGN_ONLY_PANELS`, including saved-key restoration.

**Phase 3 item 2 is implemented:** Preflight checks the current page on
demand, off the UI thread, using the separation report. Coverage is labelled
as a 72 dpi preview. Reports become stale after document or page changes;
opening a document invalidates outstanding requests. Unavailable linked
or embedded pixels now produce an error in both separation paths rather
than silently passing a partial page. Decoded linked and embedded images now feed these checks.

**Phase 3 item 1 is implemented:** live rulers in millimetres, points and
inches, selected by clicking the ruler corner. They measure from the
active page's trim and follow Design pan/zoom, independently of the raster
viewport. Fit uses the active page or spread and both canvas dimensions.
Fixing that agreement also corrected later spreads' object/guide origins
and selection outlines. Guide dragging is implemented in item 8.

### Phase 4 — output

PDF export and package actions are integrated through a separate output
window. Resolution, sequential 1/2/4-up, printer marks, hidden pages and CMYK
ICC profile are selectable. Native process channels in CMYK paints and placed
artwork are preserved. The profile is embedded as an OutputIntent; PDF/X
conformance and editable vector/text PDF are not claimed. Poppler verifies
all proof inks, media size, n-up reading order and incomplete final sheets.

Package writes layout.idml, original linked assets with collision-safe relative
names, and a manifest listing assets, fonts and warnings. Font files are not
copied. All output uses unique temporary siblings and atomic replacement.
Preflight now includes terminal overset, used missing fonts and low effective
image resolution, as well as graphics availability and ink coverage.

Remaining output work includes more external fixture/application validation,
advanced IDML text attributes and related
interchange gaps. The native debug build succeeds; development-window inspection
now covers the compact Design dock, contextual controls and undo. External
application rendering remains unverified. INDD export remains
gated by Phase 0/5. See [IDML evidence and limits](idml-format.md).

### Phase 5 — INDD

Open-ended, spike-gated. Reader for the recovered subset, then a writer if
Phase 0 justifies it.

## Known gap in the localisation

`design.lang` keys are present in English and all 149 non-English locales: **51 translated,
98 carrying English** under an explicit `# UNTRANSLATED PLACEHOLDER`
marker. The 19 new Preflight/ruler keys use English placeholders in every
non-English locale. Keys are added to all 150 catalogs at once, in English, and translation is a
separate pass — a new feature must not block on 149 translators. The strict
audit normally fails English-identical prose of seven or more words. Four
list/tab messages have exact, reasoned deferrals in
`crates/i18n/deferred-english.json`. The audit reports these untranslated
values and permits them only while Design Mode is disabled by default.
Changing the English source or enabling the feature expires the deferral;
key, placeholder and catalog checks still apply. See
[i18n translation status](i18n-translation-status.md).

## Build and check targets

```sh
make check-layout      # the kernel
make check-design      # the editor's Design Mode code
make lint-design
make check-idml        # the IDML package, cross-checked against zip/unzip
make lint-idml
make check-separation  # inks, plates, PDF
make check-i18n        # catalog checks and explicit translation-debt audit
```

`check-design` compiles the editor with `CARGO_INCREMENTAL=0`: its test
binary is large enough that the incremental cache is not worth the disk.

The i18n audit itself is `tools/check-i18n.py --strict-audit`, and adding a
key means adding it to all 150 catalogs. See
[i18n translation status](i18n-translation-status.md) for how placeholders
are marked.

## Handoff

Frame strokes and corner options, 2026-10-06:
A text frame's stroke now moves its text in by its reach into the frame, half a
centred weight, an inside weight, nothing outside or uncoloured, as InDesign's PDF
of the public paged-media `stroke-inset` sample measures on every case it sets.
Shaped frames offset their outline exactly, matching that sample's chamfered
frames to within its whole-point rounding where they had been about 4 pt further
in. Rectangles, text frames and image frames take rounded, inverse rounded, bevel
and inset corners per corner, locally or from an object style; the item keeps its
rectangle and draws, clips and sets its text in the cornered outline, and saving
writes the rectangle and its corner attributes. Decorative corners and corners on
other outlines are kept, drawn square and reported. Contour wrap around a shaped
image frame no longer shrinks to its corner.

The sample also shows InDesign fitting a frame's last line by its baseline, where
Schist needs room for its descent.

The source-frozen sweep of all 16 roadmap targets, headless library wasm, shared
UI, formatting, whitespace and the debug app build passes on Windows with **2,278
distinct passing Rust tests**, 16 new (11 layout, 2 separation, 3 IDML), and no
corrections. Before it, the frame-paint separation test's independent reference
gained the text inset its stroke now implies.

Native review used a passive capture of the actual debug app with Design enabled
and isolated configuration, on a package with a 470 × 170 pt text frame rounded
30 pt under a 6 pt centred stroke and six 120 × 80 pt rectangles: 20 pt rounded,
inverse rounded, bevel and inset corners, one with a corner of each kind and one
with a decorative corner. The canvas draws each outline with its stroke following
the corners and the decorative corner square, with one item reported unread. The
frame's first line starts 30 pt in under its corners and the others inside the
stroke, as composing the imported package directly measures (30.07, 4.54, then
3 pt). The object-style proof's four text-frame pages move their text 5 pt in for
the 10 pt stroke, matching their references, which now inset their text as much;
its image-frame pages and the other 42 proofs are byte-identical.

Next: fitting a frame's last line by its baseline, then non-solid stroke types on
items and the remaining item 9 gaps. Published to draft PR #195.


Stroke options and corpus readings, 2026-10-06:
Items now stroke with their EndCap, EndJoin, MiterLimit and StrokeAlignment,
locally or from an object style's Stroke and Corner Options category, in output
and on the canvas, and save them. Strokes mitre by default as InDesign's do;
shape strokes had joined round. Inside and outside alignment move the path half
the weight for fill and stroke alike, as InDesign's PDF of the public
paged-media `strokes-fills` sample draws a 6 pt stroke: 194 × 94 pt inside,
206 × 106 pt with square corners outside. Importing every sample of the public
corpus (59 packages built by its generator) found three misreadings, now fixed:
Lab colours in the specification's `LAB` spelling were dropped, guides on a
spread were reported as unsupported frames, and bullets remembered with their
font fell back.

The source-frozen sweep of all 16 roadmap targets, headless library wasm, shared
UI, formatting, whitespace and the debug app build passes on Windows with **2,262
distinct passing Rust tests**, 9 new (5 separation, 4 IDML), and no corrections.

Native review used a passive capture of the actual debug app with Design enabled
and isolated configuration, on a package with 12 pt strokes centred, inside and
outside three rectangles, a round-joined and a bevel-joined rectangle (the latter
filled with a spot colour defined in `LAB`), a projecting-capped line and a guide
on the spread. The canvas shows the inside-stroked rectangle smaller and the
outside one larger than the centred one, rounded corners on the round join, the
spot colour's violet, the line and the guide, with nothing reported unread. The
object-style proof's four rectangular frame pages now mitre their stroke corners
(17 pixels at 60 dpi differ, all at corners); its elliptical pages and the other
42 proofs are byte-identical.

Next: a frame's stroke moving its text, and corner options; then non-solid
stroke types on items and the remaining item 9 gaps. Published to draft PR #195.


Gradient fills, 2026-10-06:
Gradient swatches are typed from Graphic.xml (linear or radial, colour or tint
stops with locations and midpoints) and fill items whose FillColor names them,
running where the item's GradientFillStart, Length and Angle say; saving writes
them back. With no start stated a gradient begins at the path's left and bottom
edges and runs its width, which is what InDesign's own exports write (the
`multipage` fixture) and what its PDF of the public paged-media `gradients`
sample draws for items stating none. Plates, the composite and the canvas mix
each pixel's two stops; separation tests reproduce the sample's linear,
three-stop and radial pages. The `multipage` fixture's gradient no longer reports
as an unsupported colour. One key is added to all 150 catalogs, for a radial
highlight, which is not drawn.

Not composed yet: gradient strokes and text fills (still reported), radial
highlights, gradient feathers and editing gradient swatches.

The source-frozen sweep of all 16 roadmap targets, headless library wasm, shared
UI, formatting, whitespace and the debug app build passes on Windows with **2,253
distinct passing Rust tests**, 10 new (4 layout, 3 separation, 3 IDML), and no
corrections; all 43 proofs are byte-identical to the previous checkpoint.

Native review used a passive capture of the actual debug app with Design enabled
and isolated configuration, on a package laid out as the public sample: three
300 × 150 pt rectangles stating no start (linear black to paper, linear cyan to
magenta to yellow, radial paper to black) and a label frame filled at 90°. The
canvas shows the first ramp dark to light along the width, the three inks in
order with magenta at the middle, the radial gradient lightest at the bottom-left
corner, and the frame dark at its foot rising to paper; nothing is reported
unread.

Next: the remaining item 9 gaps. Published to draft PR #195.


Accurate import reports, 2026-10-06:
Importing every real template listed defaults as unread: each section's layout
name as an alternate layout, the default object styles' text-frame and effects
categories (enabled at their default values), retained text-variable
definitions and typed story structures as retained content, and a group's own
wrap and export settings as unsupported frames (they were read as page items).
The report now names only what Schist does not set: a second layout name or a
pagination master, a category whose settings differ from InDesign's defaults,
story structures still unset after reading, and a group wrap that is on (one key
added to all 150 catalogs). The PSU academic template's report falls from 22
entries to its Registration swatch, lowered local formatting and flattened
groups.

The source-frozen sweep of all 16 roadmap targets, headless library wasm, shared
UI, formatting, whitespace and the debug app build passes on Windows with **2,243
distinct passing Rust tests**, 4 new (IDML), and no corrections; one existing
object-style test now injects a category that has no default exemption. All 43
proofs are byte-identical to the previous checkpoint. Opening the PSU academic
template in the actual debug app (Design enabled, isolated configuration) now
shows "7 unread" in the status bar, down from 22: its Registration swatch,
lowered local formatting, four flattened groups and the implicit tab note.

Next: gradient swatches (linear and radial fills, as the public paged-media
`gradients` sample shows), then the remaining item 9 gaps. Published to draft
PR #195.


Table and cell styles, 2026-10-06:
Tables now resolve their settings through their table style and cell styles,
with BasedOn chains, as the IDML specification lays them out: a cell's own
attributes, then its applied cell style, then the cell style its table region
(header, footer, left or right column, body) gives it, their bases and `[None]`.
Insets, vertical justification, fills and edge strokes come through that
cascade. Fills alternate by row or column as the table or its style asks, row
patterns skipping header and footer rows, and ColumnFillsPriority picks which
axis shows; a fill a cell or cell style gives, none included, wins. InDesign's
PDF of the public paged-media `tables` sample fills rows 1 and 3 of three at its
default 20 % tint and draws no fills for a style giving only column fills; its
`styles-cascade` PDF leaves plain a table whose styles link their bases in a
BasedOn attribute, which Schist now ignores as InDesign does. The table and
cell style groups are retained and saved unchanged, so a reopened table resolves
the same; until now saving dropped them.

Not composed yet: StartRow, alternating strokes, table borders, table space
before and after, cell styles' paragraph styles, diagonals, rotated cells,
non-solid strokes and editing tables and their styles.

The source-frozen sweep of all 16 roadmap targets, headless library wasm, shared
UI, formatting, whitespace and the debug app build passes on Windows with **2,239
distinct passing Rust tests**, 3 new (2 layout, 1 IDML), and no corrections; all
43 proofs are byte-identical to the previous checkpoint. The full IDML suite,
real templates' round trips included, passes with their style groups retained.

Native review used a passive capture of the actual debug app with Design enabled
and isolated configuration, on a package whose table style alternates a 25 %
black row with a plain one, defines column fills that ColumnFillsPriority hides,
and gives the header region a 60 % cell style centred in a 34 pt row; one body
cell sets its own fill to none. The capture shows the dark header with centred
text, rows 1, 3, 5 and 7 striped and the others plain, the opted-out cell white
in its striped row, and no column fills.

Next: the remaining item 9 gaps. Published to draft PR #195.


Tables breaking across frames, 2026-10-06:
A table that does not fit the room left now breaks between whole rows into
parts, each set on a line of its own, the next starting at the top of the next
column of the thread. Header rows repeat in every column, once per frame or once
per page (BreakHeaders), footer rows likewise (BreakFooters), and the first
header and last footer can be skipped; rows kept with the next (KeepWithNextRow)
never end a part unless nothing else would fill a whole column, a break never
falls inside a cell spanning rows, and a row no column holds leaves the rest of
the table overset. The rules reproduce InDesign's PDF of the public paged-media
`tables-rows` sample row for row on its three frame-break pages (header + rows
1-5 then header + rows 6-10; keeps moving rows 3-7 on together; a 179 pt row
left overset with an empty second frame), and the `tables-overset` sample's five
frame heights (two of four rows in 62 pt, none in 20 pt, nothing when only the
header would fit). That sample's 361 pt table overhangs its 360 pt frame, which
Schist had refused to set at all: a table wider than its column now overhangs
it. Because where the parts fall depends
on where composition sets them, a story holding tables composes again until its
parts settle (`table_flow`): the first part fills the room measured below the
line before it, and a part that moves past its planned column caps that column
for the next plan. A table never shares a line with text (text before ends the
line above, text after starts the line below), and its lines step down from the
text above by the text's extra leading under fixed leading too, so a table never
overlaps it. Row StartRow other than Anywhere and undefined repeat values are
reported and not applied: one key is added to all 150 catalogs.

Not composed yet: StartRow, table and cell styles (including `[Basic Table]`),
table space before and after, alternating fills and strokes, diagonals, rotated
cells, non-solid strokes and editing table structure.

The source-frozen sweep of all 16 roadmap targets, headless library wasm, shared
UI, formatting, whitespace and the debug app build passes on Windows with **2,236
distinct passing Rust tests**, 11 new (9 layout, 1 IDML, 1 separation), and no
corrections; all 43 proofs are byte-identical to the previous checkpoint. A first
sweep was stopped early to add the `tables-overset` cases and the overhang fix
they exposed, and the whole sweep ran again on the final source.

Native review used passive captures of the actual debug app with Design enabled
and isolated configuration, on a package holding two threaded 340 pt frames: an
intro line, then a table of a tinted header row, 24 body rows of one to three
lines (rows 9 to 11 kept with the next) and a tinted footer row, then a line
after the table. The first frame shows the header, rows 1 to 7 and the footer;
the second repeats the header and shows rows 8 to 14, the kept rows moved on
with row 12, and the footer, with the frame's overset marker for the rows and
text that do not fit. The first build placed no part at all: the default body
style keeps a paragraph's first and last two lines together, and a table's
parts counted as lines of its paragraph. Keep options no longer hold table
parts (`fitting_lines` takes whether the paragraph sets a table), and the new
layout tests cover it. A forced line break inside an inline item's atomic span
had also made the engine refuse the line; only the item itself is atomic now. A
final passive capture with the swept build shows the same two frames.

Next: table and cell styles with alternating fills and strokes, then the
remaining item 9 gaps. Published to draft PR #195.


Tables, first phase, 2026-10-05:
A Table inside a story is now typed from its retained XML (`tables::Table`, the
new `StoryStructure::table`): header, body and footer counts, row sizing, column
widths, and cells with spans, fills, edge strokes, insets and vertical
justification. Each cell's paragraphs become a document story, lowered to styles
like any story, so cell text composes with the ordinary machinery and can be
edited in the Story Editor. A table is set in its line as one block the size of
its grid; its outer stroke edge sits at the line's top left, rows grow to their
cells' text, and output and the canvas draw fills, edges and cell text with the
holding frame (cell text as generated text of that frame on the canvas). The
row-height rule, default insets and edge drawing follow InDesign's PDFs of the
public paged-media `tables` and `tables-rows` samples: a growing row is its top
inset, its tallest cell's last baseline below the content top, and its bottom
inset (20.826 and 49.626 pt for one and three 12 pt Open Sans lines), and
horizontal edges reach the table's outer edge while vertical edges stop at the
horizontal strokes. One key (an unreadable table) is added to all 150 catalogs;
the PSU academic template's 7-column table is now typed.

Not composed yet: tables breaking across frames with repeated header and footer
rows, table and cell styles (including `[Basic Table]`), alternating fills and
strokes, diagonals, rotated cells, non-solid strokes and editing table structure.
These are the next phases; the `tables-rows` and `tables` PDFs cover them.

The source-frozen sweep passed every target but the i18n audit, which flagged the
new message as seven or more untranslated English words in the major locales. It
was shortened to "Table {name} unread: inconsistent grid" in all 150 catalogs
rather than deferred, and the i18n check then passed. Every other roadmap target,
headless library wasm, shared UI, formatting, whitespace and the debug app build
passed, and all 43 proofs are byte-identical to the previous checkpoint: **2,225
distinct passing Rust tests**, 11 new (8 layout, 2 IDML, 1 separation). The PSU
academic template's table imports typed: 7 columns, 12 rows, a header row and 84
cells.

Native review used passive captures of the actual debug app with Design enabled
and isolated configuration, on a package holding a native-style 3 × 3 table with
a tinted header row, cells of one to three paragraphs and a bottom-justified cell
in a 70 pt row. The first capture showed header text and a cell's third line
missing: a row as tall as its last baseline left no room for that line's
descent, so the cell frame dropped the line. Cell text frames now extend below
the row by their last line's descent, and a layout test checks every line of
every cell is drawn; the same review also caught measurement subtracting the side
insets twice. The final capture shows the header text on its tint, all three
lines, and "bottom 2" at its row's foot. At 43% the canvas draws 1 pt grid lines
faintly; a 144 dpi plate render shows every edge and all cell text.

Next: tables breaking across frames with repeated headers, then table and cell
styles. Published to draft PR #195.


File-name and date variables, 2026-10-05:
CreationDateType, ModificationDateType, OutputDateType and FileNameType
definitions are now typed when their preferences state their settings, and save
with InDesign's spellings; every public template's default definitions now type.
Documents carry creation, modification and output dates (`DocumentDates`) and
their file path. Creation and modification dates are read from and written to
the package's XMP; the editor stamps a missing creation date and the session's
output date when a document opens, the modification date and file path on save,
and the output date on the copy each PDF or package output renders. A new
`dates` module parses ISO 8601/XMP dates and formats InDesign's date codes,
including the uppercase year its own default "Output Date and Time" writes. The
public `variables` sample's three formats print exactly as in InDesign's PDF.
The Text Variables window lists date and file-name definitions by kind without
editing them; four keys are added to all 150 catalogs.

The source-frozen sweep passed every roadmap target, headless library wasm, shared
UI, formatting, whitespace and the debug app build with no corrections (two
fixture expectations had been updated before it for the newly typed default
definitions); all 43 proofs are byte-identical to the previous checkpoint:
**2,214 distinct passing Rust tests**, 9 new (6 layout, 3 IDML).

Native review used a passive capture of the actual debug app with Design enabled
and isolated configuration: a document carrying XMP dates shows "Created
2026-10-01", "Modified October 2, 2026", "Output Monday 05.10.26" (the day it was
opened) and "File dates.idml".

Next: tables, then the remaining item 9 gaps. Published to draft PR #195.


Running headers and jump numbers inside threads, 2026-10-05:
MatchParagraphStyleType and MatchCharacterStyleType definitions are now typed
(`text_variables::RunningHeader`) when their preference states the style,
SearchStrategy, ChangeCase and DeleteEndPunctuation as InDesign writes them,
and save with those spellings; style references are named after styles are read.
An instance shows, for the one page it is set on, the first or last paragraph in
the style starting there or run in the character style starting there, ordered
by frame and line, and carries the previous page's value forward when its page
has none. Change case and end punctuation follow InDesign's PDF of the public
paged-media `variables` sample, and a layout test reproduces that sample: all
eight headers on all five pages (labels 1 2 1 2 3) match InDesign's text,
including page 2's first-on-page header carrying page 1's first heading and the
title case "Part Two Begins (a Third Heading)". Stories that show headers are not
searched, so evaluation never recurses. The Text Variables window lists running
headers by kind and style without editing them; one key is added to all 150
catalogs.

Next and previous page numbers inside a multi-frame thread now render from the
frame they land in: composition runs again with each marker's frame until none
moves. They follow their own thread, so the sample's story threaded from page 3
to page 5 prints "continued on page 3, previous 1" and "continued from page 1,
next 3" exactly as InDesign does; one-frame stories keep the touched-story rule.

The source-frozen sweep found three things, all corrected and re-passed on the
final source: two clippy findings in the new code (a complex tuple type, now an
alias, and `last` on a double-ended iterator, now `rfind`), which failed every
lint target, and two IDML tests that listed the public fixtures' typed variables:
every public fixture carries InDesign's default "Running Header"
(MatchParagraphStyleType on NormalParagraphStyle), which is now typed, so both
expectations gained it. The layout and IDML targets and all lints then passed
again; every other roadmap target, headless library wasm, shared UI, formatting,
whitespace and the debug app build passed in the sweep, and all 43 proofs are
byte-identical to the previous checkpoint: **2,205 distinct passing Rust tests**,
4 new (2 layout, 2 IDML).

Native review used a passive capture of the actual debug app with Design enabled
and isolated configuration: three pages with header frames showing a
first-on-page heading as is and in upper case with end punctuation removed. Page
1 shows "first page heading" and "FIRST PAGE HEADING", page 2 (no heading) carries
them forward, and page 3 shows "Third page heading" and "THIRD PAGE HEADING".

Next: file-name and date variables (document dates, output time and the file
path), then tables and the remaining item 9 gaps. Published to draft PR #195.


Jump-line page numbers, object-style wrap and layer IgnoreWrap, 2026-10-05:
Next and previous page numbers now render where public jump-line guidance makes
them unambiguous (CreativePro, linked in `docs/idml-format.md`): in a composition
pass with one document frame, the marker shows the page of the next or previous
frame in the thread of the story whose frame its own frame touches or overlaps,
and its own page when standalone or at that thread's end. Markers inside a
multi-frame thread, on parent pages, or touching stories that disagree stay
diagnosed. The Text Variables window gains next and previous icons beside the
current page number (two new icons; the existing marker labels serve as
tooltips).

An ObjectStyle's Text Wrap & Other category is now read, written and applied:
EnableTextWrapAndOthers and the style's TextWrapPreference, inherited through
BasedOn. Items without their own wrap take the style's (`StyleSet::object_wrap`,
used by composition, the wrap controls and the canvas boundary); editing a styled
item's wrap stores it only where it differs from the style's. Nonprinting, the
category's other member, keeps the unsupported-category report. Each Layers panel
row gains a compact toggle for the layer's IgnoreWrap. No new keys.

The source-frozen sweep passed every target except the IDML tests, where two
older tests still expected next and previous page numbers in a standalone frame to
stay unrendered; under the new rule they show the frame's own page. Both
expectations were corrected (test files only), after which the IDML target and its
lint passed again on the final source. Every other roadmap target, headless
library wasm, shared UI, formatting, whitespace and the debug app build passed,
and all 43 proofs are byte-identical to the previous checkpoint: **2,201 distinct
passing Rust tests**, 3 new (2 layout, 1 IDML).

Native review used the actual debug app with Design enabled and isolated
configuration. Passive captures show a jump line touching the first frame of a
three-page thread reading "Next page 2 previous page 1", and text flowing on both
sides of a box whose wrap comes only from its object style. With guarded input
(sent only when a Schist window is under the pointer or in front): the Layers row
shows eye, lock and the new wrap toggle; turning it on made the frame's text run
over the box and marked the document modified. The Text Variables window shows
the four marker icons compactly, and after capturing a text cursor the next-page
icon inserted a marker listed as "Next page number".

Next: file-name, date and running-header variables, then tables and the
remaining item 9 gaps. Published to draft PR #195.


Text wrap around custom-positioned anchored items, 2026-10-05:
An item at a custom position with a TextWrapPreference now wraps the lines of its
own story after its anchor's line, as Adobe's public help and tutorials describe
(sources in `docs/idml-format.md`): from that line's bottom in the anchor's frame
and wholly in later frames of the thread on the same page, never the anchor's
line or earlier lines. Wrap obstacles gained a first band (`Obstacle::from`), and
obstacle construction is shared by page items and anchored items
(`WrapField::for_frame_with`). A projected story's composition runs again with
its items where the previous pass put them until they settle, at most four more
passes; stories without such items compose once as before. Preflight now warns
"wrap not applied to some text" on another story's frame that an item's wrap
reaches, rather than on the item's own frame; inline items' contour, jump and
column wraps keep that warning. Parent-page instances do not apply anchored wrap.
No new keys.

The source-frozen sweep passed every roadmap target, headless library wasm, shared
UI, formatting, whitespace and the debug app build with no corrections; all 43
proofs are byte-identical to the previous checkpoint: **2,198 distinct passing
Rust tests**, 3 new (2 layout, 1 separation; one layout test now covers applied
and unapplied wraps).

Native review used a passive capture of the actual debug app with Design enabled
and isolated configuration: a 120 × 110 pt box anchored at a sentence's end, at
the frame's left edge with its top on the anchor line's baseline and 10 pt right
and 8 pt bottom wrap offsets. The anchor's line keeps the full measure, the six
lines beside the box start right of it and its offset, and the lines below
return to the full width.

Next: the remaining item 9 gaps (object-style wrap categories, next/previous page
numbers, file-name/date/running-header variables, tables). Published to draft
PR #195.


Anchored text frames and groups, 2026-10-05:
A TextFrame inside a story is now typed with the package story its ParentStory
names. That story composes in the item's own box as a one-frame thread (the
existing standalone path for frames outside `doc.objects`), its own anchored
items included; output paints it, Preflight reports its overset text under the
item's name, and the canvas draws its lines as generated text of the holding
frame, so a click never edits them as that frame's story. An anchored frame whose
story leads back to the story holding it, directly or through other anchored
frames, is reported as an invalid ParentStory and left untyped, and composition
never sets one. A Group inside a story is flattened like spread groups (nested
ItemTransforms compose, opacity multiplies, hidden groups hide their items) into
`AnchoredItem::members`; members move together and the extent spans them, while
the container keeps the group's name and text wrap. No new keys.

Placed items take a reserved object id that no document object has, so a typed
item restored from Schist's structured-story retention record with an id saved
in another session never composes as a document frame. Correction to the two previous entries: anchored items are saved in that
record with their retained XML, not written back as native story content, so
other applications do not see them.

The first source-frozen sweep passed everything except one IDML test: refreshing
anchored item ids on every import made a public fixture's stories differ after a
save. The ids are no longer touched; placed copies take the reserved id instead,
and a test now checks stories are unchanged by saves. The full sweep then passed
again on the corrected, frozen source: every roadmap target, headless library
wasm, shared UI, formatting, whitespace and the debug app build, with all 43
proofs byte-identical to the previous checkpoint: **2,195 distinct passing Rust
tests**, 9 new (5 layout, 3 IDML, 1 separation).

Native review used passive captures of the actual debug app with Design enabled
and isolated configuration: a 130 × 48 pt anchored text frame set inline shows
its own 9 pt story wrapped inside it with its line raised to make room, and a
group of a box and a circle anchored at a custom position stands 10 pt right of
the frame beside its anchor line with the circle 26 pt below the box.

Next: text wrap around custom-positioned anchored items, then the other item 9
gaps. Published to draft PR #195.


Above-line and custom anchored positions, 2026-10-05:
Every AnchoredObjectSetting attribute in the public specification is now typed
with its Appendix C default (`anchored::Placement`): anchor point, horizontal and
vertical alignment and reference point, X offset, space above, spine-relative,
pinned and locked. An invalid reference, alignment, anchor point or flag is
reported and read as its default; an invalid position or offset still leaves the
item untyped. No new keys.

Engine inline boxes gained room above their line (`InlineBox::above`): the line's
ascent and step grow by its boxes' rooms, so the line and every line after it move
down, the first baseline included. A box with no height takes its character's
font metrics. `inline_box_positions` also reports the box character's size,
ascent, OS/2 cap height and x-height.

An item above the line is a zero-width box whose room is its space above, its
visual height and its Y offset (the space after it). The item's stroked bottom
sits its Y offset plus half the em plus half the cap height above the lowered
baseline; several items stack in anchor order. It is aligned left, center or right
in the line's column, by the paragraph's alignment for TextAlign, mirrored on
left-hand pages when spine-relative. A custom item is a zero-width box that takes
no room: one of its nine stroked-extent points is put at the anchor's pen
position or the left, center or right of the column, frame, page margins or page,
and at the anchor line's baseline, cap height, x-height, ascent or top of leading
or the top, center or bottom of the column, frame, margins or page. The Y offset
moves it down; the X offset moves it away from its aligned side. Pinned
line-relative items stay between the frame's top and bottom. Positions are
computed in the frame's own space and drawn through its affine. A composed item
whose wrap is not applied (any wrap of a custom item; contour, jump or column
wrap of an inline item) raises "wrap not applied to some text"; only items in
vertical text and anchored text frames and groups remain unrendered.

Evidence is the same pinned paged-media sample's InDesign 20 PDF: above-line
step 50.9 pt (14.4 + 36.5) with the stroked bottom 10.2832 pt above the baseline,
exactly (12 + 8.5664) / 2 for Open Sans's cap height; AnchorLocation with LeftAlign
and offsets 24/12 putting the top left 24 pt left of the pen and 12 pt below the
baseline with the anchor taking no width; frame top right, line baseline, top of
leading (the previous baseline) and page-margin bottom right exact to the stroked
extents. The sample's `AnchorLocation`, `LineCapHeight` and `LineXHeight`
vertical references are not specification values and their pages equal the
baseline page, so InDesign read them as the default; Schist does the same. Not
covered natively, and so Schist readings: the above-line split for other fonts and
its spaces, X offset direction for other alignments, cap-height, x-height and
ascent references, pinning, spine mirroring, column and page-edge references, and
page references for rotated frames. A new layout test file reproduces the native
pages with IBM Plex Sans metrics.

The source-frozen sweep passed every roadmap target, headless library wasm, shared
UI, formatting, whitespace and the debug app build with no corrections; all 43
proofs are byte-identical to the inline-item checkpoint: **2,186 distinct passing
Rust tests**, 10 new (2 text engine, 6 layout, 2 IDML; the separation item test
now covers all three positions).

Native review used passive captures of the actual debug app with Design enabled
and isolated configuration. A document with a banner above the second paragraph's
first line (centered, 4 pt above, 2 pt below), an oval 8 pt right of the frame at
its anchor line and a box 24 pt left of its anchor and 12 pt below the baseline
shows each where specified, with the banner's line moved down to make room.

Next: anchored text frames and groups, then text wrap around custom-positioned
items. Published to draft PR #195.


Inline anchored items, 2026-10-05:
A Rectangle, Oval, Polygon or GraphicLine inside a story is still retained as its
exact XML, which saving writes unchanged, and is now also typed from it: the
AnchoredObjectSetting position (InlinePosition when absent, AboveLine, Anchored),
AnchorYoffset and the page item itself, read by the spread-item reader. Invalid
settings are reported and leave the item untyped (one key, all 150 catalogs).

The text engine gained inline boxes: a U+FFFC inside an isolated inline object may
carry a width, ascent and descent. Shaping advances by the width and draws
nothing, line metrics treat it as a glyph of that height, it wraps whole, and
`inline_box_positions` reports where it was set, in either direction.
Composition projects each inline item in horizontal text as such a box with a
generated character style, so source text and anchors are untouched. The box is
the item's visual extent (its frame grown by half its stroke), whose bottom sits
on the baseline raised by its Y offset; a bounding-box wrap adds its left and right
offsets beside it. The item is drawn through the frame's affine after the frame's
text, in output and on the canvas, where a click on it selects the frame. Under
font-metric leading a tall item raises its line like a large glyph. Schist
resolves Auto leading to points before the engine, so the item's run carries the
item's height above the baseline plus the text's extra leading; fixed leading
keeps its step and the item overlaps the line above. Composed inline items no
longer count as unrendered structures.

These rules follow InDesign's own output: the pinned public paged-media
`anchored` sample and its InDesign 20 PDF (hashes in `docs/idml-format.md`) put
a 60 × 36 pt frame stroked 0.5 pt inline in 12 pt Auto-leaded text. Its content
stream steps the anchor line 38.9 pt (36.5 + 14.4 − 12), puts the stroke's outer
edge on the baseline and at the pen position, and on the wrapped page moves the
frame 3 pt and the following text 6 pt with no baseline moving. A layout test
reproduces those numbers. Its other pages (above-line, custom offsets and text
frame, line, top-of-leading and page-margin references) measure exactly against
the visual bounds too and are the evidence for the next batch; its cap-height and
x-height pages use enumeration spellings InDesign evidently ignored, since they
match the baseline page. A nonzero inline Y offset remains a Schist reading.

Not composed yet, and still reported by Preflight: above-line items, custom
anchored positions, items in vertical text, anchored text frames and groups (still
retained and untyped), and text wrap around anchored items beyond an inline
item's own side offsets. Inline items are not editable; they save as their
retained XML.

Focused testing found that every story-structure literal needed the new field
(41 across the repository, two helper bodies fixed by hand) and that two TextSpec
literals needed the box list. Native review then found two composition bugs, both
fixed with tests. Line specs were scaled to the preview zoom and to output
resolution without their boxes, so a line raised by a tall item set its text at
the wrong height except at 100% and 72 dpi; boxes now scale with their specs at
all three sites, and tests check the preview at 0.48 and 2.0 and plate ink at 72
and 144 dpi. And Auto leading never reached the box: IDML's root paragraph style is
Auto, so every imported review story showed a tall item overlapping the lines
above instead of making room for it. A first fix used 120% of the item's height;
the native sample then showed InDesign's rule and that its extent includes the
stroke, so a sweep already under way was stopped and both were corrected before
the final sweep. On the frozen final source every roadmap target, headless
library wasm, shared UI, whitespace and the debug app build passed; the format
check failed on rustfmt layout in the new layout test file only, which was
formatted, after which the format check, the layout target and its lint passed
again. All 43 proofs are byte-identical to the shaped-frame checkpoint:
**2,176 distinct passing Rust tests**, 19 new (5 text engine, 8 layout, 3 IDML,
3 separation).

Native review used the actual debug app with Design enabled and isolated
configuration, through passive captures only. A centered 18 pt story with a filled
36 × 14 pt rectangle and a 30 × 44 pt oval raised 6 pt shows the rectangle on its
baseline between words and, under the root Auto leading, the oval's line dropped
to make room for it, the oval 6 pt above the baseline; with a fixed 22 pt
leading the step is unchanged and the oval overlaps the two lines above. On the
final build the oval's line steps 53.6 pt (its 50 pt above the baseline plus the
18 pt text's 3.6 pt extra leading), so the oval sits just under the line before.

Next: above-line and custom anchored positions, then anchored text frames and
groups. Published to draft PR #195.


Text in shaped frames and IDML root styles, 2026-10-05:
A text frame with a non-rectangular outline now composes inside it instead of in
its rectangle with a "rectangular text composition" notice (the notice and its key
are gone). The outline is an inverse wrap of the frame's own shape in its
untransformed box, inset by its top inset (InDesign offers one inset for such
frames), so it shares the text-wrap bands: lines take the intervals the outline
leaves across their whole height, holes follow the path's fill rule, other items'
wrap still applies, and ignoring wrap never removes a frame's own shape. Text the
shape cannot hold is overset and threads on; vertical text, initials and list
markers keep the rectangle with the Preflight wrap warning. IDML frame geometry
has no fill rule, so saving an even-odd outline warns. "Text in shape" beside
"Text on path" turns a selected closed shape into such a frame in one undo step,
keeping its paint; a rectangle becomes an ordinary frame. One key is added to all
150 catalogs.

Unstyled runs, controls and list markers now save as
`CharacterStyle/$ID/[No character style]` instead of the undefined
`CharacterStyle/$ID/`, and Styles.xml always defines both root styles (bare when the
document has no style of that name), so every style a saved story or style names
resolves inside the package. On reading, the root character style and a bare root
paragraph style are not document styles, and any spelling of the root character
style (including a BasedOn of `$ID/[No character style]`) means no style. This
deliberately changes two things: InDesign fixtures no longer list
`[No character style]` as a document style, and controls in unstyled native text
import with the empty style rather than that name (one expectation updated).
Structured-story and automatic-direction guards compare with the same spellings.

Focused testing found: normalized outlines were flattened before scaling, so a
0.25 pt tolerance on a 1 pt circle produced a diamond (this also affected contour
wraps around shaped text frames); the bare root paragraph style needed detection
from its XML rather than its parsed properties; the structured-story rename check,
a fixture's `$ID/[No character style]` BasedOn and the automatic-direction label
needed the root spelling. The source-frozen sweep then failed four steps on two
test-side mistakes, which were corrected and those targets re-run on the final
source: the new shared UI test lacked an import (shared UI, Design and workspace
lints), and the object-style proof built its independent reference with a
rectangular text frame (check-design-output). That proof's two curved-text pages
now match the reference with the text inside the ellipse; the other 42 proofs are
byte-identical to the text-wrap checkpoint. Every other roadmap target, headless
library wasm, formatting, whitespace and the debug app build pass:
**2,157 distinct passing Rust tests**, 13 new (8 layout, 3 IDML, 1 separation, 1
shared UI).

Native review used the actual debug app with Design enabled and isolated
configuration. Passive captures of documents with an elliptical frame and one with
an elliptical hole show text following the outline and flowing around the hole
in reading order. Selecting a filled ellipse shows "Text on path" and "Text in
shape" on one compact row. Review then found two problems in typing, both fixed:
after either attach button, keyboard focus stayed on the button, so typed keys
acted as tool shortcuts; both buttons now give the canvas focus. And on Windows,
spaces typed into Design text frames were dropped (also in `550ba45f`): GPUI's
Windows backend reports Space by name without its character, so the typing branch
ignored it. Design typing and the shared line editor (Story Editor and other
`LineEdit` users) now treat it as a space; panel fields already did. On the final
build, "Text in shape" followed by typing produced spaced text inside the ellipse,
narrow at its top, and the saved IDML holds it. Composing the long hyphenated
wrap-review story takes about 3 s in the debug build with or without wrap, so
typing there lags in debug; this is not specific to wrap.

Next: anchored and inline objects, which need an engine inline box (a replacement
character with its own advance and line metrics). Published to draft PR #195.


Text wrap, 2026-10-04:
Page items keep a typed TextWrapPreference: mode (bounding box, contour, jump
object, next column), side, Inverse, ApplyToMasterPageOnly, four offsets and the
ContourOption (type, inside edges, path name). Text frames and layers keep
IgnoreWrap; the document keeps AbutTextToTextWrap, ZOrderTextWrap and
JustifyTextWraps. All of it saves with native spellings through repeated saves,
including parent spreads; invalid values are reported and read as the published
defaults. No public fixture uses wrap, and the pinned paged-media `text-wrap`
sample's PDF never reaches its obstacles, so geometry follows Adobe's published
wrap documentation; inferences are listed in [IDML evidence and limits](idml-format.md).

Horizontal text composes around bounding boxes (page-aligned bounds plus four
offsets) and contours (shape path, clipping path or text-frame outline, plus one
offset), and honours jump-object, next-column, sides, Largest Area (decided once
per column), spine sides by page position, inverse outlines (text inside the
narrowest interior), ignore-wrap frames and layers, master-only parent items,
stacking order for wrap-beneath-only, and Abut's leading increments. Rotated or
skewed frames and items wrap in page space, and artwork owned by the facing page
wraps across the gutter. Each free interval of a line band is one measure for the
existing per-line engine, so lines fill both sides of an object in reading order;
band heights feed back until stable, a line never takes a band its real height no
longer leaves free, and an interval an obstacle narrowed that its line cannot fit
is skipped rather than overflowed. Vertical text, initials, list markers and path
text compose unwrapped with a Preflight warning; pixel contours use the item's
outline with a Preflight warning.

The Control panel adds five mode icons, an ignore-wrap toggle for text frames, a
uniform offset field, an inside-outline toggle and a side dropdown; Text
Preferences adds two document toggles. Each gesture is one undo step. The canvas
draws a dashed boundary for selected box-shaped wraps. 21 short keys are in all
150 catalogs.

Focused testing found and fixed: a band touching an obstacle's bottom edge counted
as intersecting it, pushing non-abutting text a full line lower; the layer reader
lacked its report. Lints asked for `as_chunks` and a test type alias. A first
source-frozen sweep passed (2,143 tests), but native review then showed that a
hyphenated paragraph whose first word did not fit a narrowed interval was rejected
whole (and a plain word would instead have overflowed into the wrap). Narrowed
intervals a line cannot fit are now skipped and the paragraph re-planned (at most
64 per paragraph); a regression property covers hyphenated and centered text in an
inverse ellipse and beside a 20 pt gap. A second sweep passed with it (2,144), and
all 43 proof PDFs were byte-identical to the first sweep's. Native review also
showed multi-second pauses in the debug build after a wrap edit: each wrapped
paragraph was shaped once unwrapped only to estimate line heights. A one-character
metric sample now seeds the plan and refinement corrects mixed sizes; on the review
page a debug composition fell from 1.08 s to 0.15 s (contour) with identical line
counts.

The final source-frozen sweep passes all 16 roadmap targets, headless library
wasm, shared UI, formatting, the staged whitespace check and the debug app build:
**2,144 distinct passing Rust tests**. Twenty-two new properties pass (15
layout, 4 IDML, 2 separation, 1 editor). Its formatting step flagged a throwaway
debugging test left in the tree; the file was removed (it was never part of the
batch) and formatting, the whitespace check and `check-idml` passed again.

Native review used the actual debug app with Design enabled and isolated
HOME/APPDATA/LOCALAPPDATA/XDG directories. On the first build, the Properties panel
showed the Text wrap row; contour wrap flowed the review story around both sides
of an ellipse in reading order; an 8 pt offset widened the gap; the six-way side
dropdown and Right side kept text to one side; and the frame's ignore-wrap toggle
appeared beside the modes. On the final build, documents carrying each native
wrap preset (contour, inside-outline, right-side bounding box, jump object, next
column in two columns, and an ignoring frame) open and render as specified;
inside-outline text fills the ellipse with hyphenation and the rest is overset.
The review desktop turned out to be shared with another application being driven
at the same time, so interactive input on the final build was stopped after a
guarded check, and those renders were captured from Schist's own window without
sending input. An opaque item stacked above its frame hides inside-outline text,
as expected from paint order.

Next is text in shaped frames, which reuses the wrap bands for a frame's own
outline. Published to draft PR #195.


Page numbers, section markers and chapter numbers, 2026-10-04:
Native `<?ACE 18?>` page numbers (current, next and previous by their range's
PageNumberType) and `<?ACE 19?>` section markers are now typed zero-width
controls. They save back as native instructions, upgrade from older
recovery-only records after the story guard agrees, follow style renames in one
undo step, and yield to external native edits. Current page numbers show the
page's Pages-panel label and section markers their marker text, using the same
page context as last page numbers: a parent instance's destination page, or an
ordinary thread only when all its frames share the page (or section). Next and
previous page numbers need the adjacent frames of the marker's own frame, which
composition does not yet supply, so they save natively but stay diagnosed.

ChapterNumberPreference, previously dropped on every save, is retained; invalid
numbers or sources are reported. ChapterNumberType definitions lower like last
page numbers. With no preference Schist uses chapter 1, as InDesign's application
default renders in the public reference; UserDefined uses its number; the
book-relative sources render only chapter 1, where a standalone document's
possible readings agree. All public fixtures' native Chapter Number definitions
are now typed. Variable authoring now goes through a single `VariableKind`.

The compact Text Variables window gains a chapter kind (Format only) and two
icons that insert a current page number or section marker at the captured cursor,
each one undo step; markers appear beside variable instances there for exact
removal. Insertions at the end of a run take the preceding character's style, as
typed text would; native review exposed the previous paragraph-default fallback.
Five short keys are in all 150 catalogs; three new icons are registered.

Native review also found that closing the main window with unsaved Design
changes quit without a prompt on Windows; the `aaad6dc0` build behaves the same.
GPUI runs the close hook with the window already leased, so the hook's nested
window update failed and its fallback allowed the close. The hook now reaches the
workspace through the leased window. Natively, Alt+F4 on a dirty layout shows
Unsaved changes; Cancel keeps it (and an open variable window) open, Don't Save
quits, and Save writes the edit before quitting.

Eighteen new properties pass (nine layout, nine IDML). One earlier IDML test was
superseded and several expectations changed deliberately: recovery-only page
instructions are now typed and native, and fixture Chapter Number definitions are
typed. The source-frozen sweep passes all 16 roadmap targets, shared UI,
formatting and whitespace: **2,120 distinct passing Rust tests**, four browser
checks and eight Python audit tests (one existing shared UI documentation example
ignored). A clippy `matches!` finding stopped the first sweep attempt before
anything was counted; the fixed tree was linted workspace-wide before restarting.
The insertion-style fix and the close-hook fix followed native review: layout,
IDML and Design tests and lints, workspace clippy, the app and web checks,
formatting, whitespace and the app build pass again on the final source, with two
new properties (**2,122 tests**). The variable proof now has 36 pages: its first
24 are pixel-identical to the previous proof, the 12 page-number/section-marker
pages match their literal controls in Poppler and pass visual review, and the
other 42 proofs are byte-identical. The debug app builds in 9m 21s.

Native review with Design enabled and isolated configuration shows the fixture's
native Chapter and Last Page definitions typed; the window's chapter kind with only
a Format dropdown; page-number and section-marker icons inserting at the captured
cursor with removable rows; page numbers following section edits (I → 5 → 1)
through one-step undos; native `<?ACE 18?>`/AutoPageNumber output and a preserved
ChapterNumberPreference after save; and the process ending when the main window
closes with the variable window open.

Follow-ups found: Schist writes unstyled runs (and unstyled inserted objects) as
`CharacterStyle/$ID/` rather than `[No character style]`; this pre-existing
exporter behavior affects all text and needs its own verified change. Next is
text wrap: public paged-media `text-wrap` output does not reach its obstacles,
so wrap geometry follows Adobe's published wrap documentation and the engine's
existing per-line measures. Published to draft PR #195.


Last page number variables and resumed validation, 2026-10-04:
Work moved to a Windows Server machine. The interrupted `aaad6dc0` validation was
rerun from an isolated LF worktree before any new code compiled:
`check-design-output`, `check-editable-interchange`, `lint-editable-interchange`,
headless `check-library-wasm`, shared UI, formatting and the commit's whitespace
check all pass, and the debug app builds in 35m 48s. With the twelve targets
completed before the handoff, that checkpoint has **2,095 distinct passing Rust
tests**. All 43 proof PDFs are byte-identical to a `b0cdef3c` baseline regenerated
in a separate worktree on the same machine. An earlier attempt that overlapped new
source edits was discarded rather than counted.

Native review of the committed window used the actual debug app with Design enabled
and isolated HOME/APPDATA/LOCALAPPDATA/XDG directories. Type → Text Variables and
the Stories toolbar open it; New/Edit/Save, insertion, shared edits, one-step
undo/redo, recapture, stale-cursor and closed-document refusal, coincident instance
removal, used-definition delete refusal and IDML save/reopen pass. A 380 px window
keeps every control reachable. Review found that closing the main window left an
open variable window as an inert orphan that kept the Windows process alive.

LastPageNumberType definitions are now typed. Explicit Format and Scope values from
the published PageNumberVariablePreference subset lower and save with their native
spellings; Current, Arabic, Roman and letter formats render. Kanji, full-width,
leading-zero, absent or unknown values remain exact recovery data. A parent
instance is evaluated for its destination page, so one footer shows each section's
own value; an ordinary thread renders a section value only when all of its frames
share one section. Otherwise the existing retained-structure Preflight error
applies; visible section prefixes are not guessed. The pinned public InDesign 20
reference's labels (3; 2 then 3; III) are reproduced.

The compact window adds custom/last-page icon toggles. Last-page drafts show only
Text before/after plus Format and Scope dropdowns; each kind keeps its own fields.
Seven short keys and a corrected New tooltip are in all 150 catalogs; number
formats reuse list labels. Closing the main window now also closes the Story Editor
and Text Variables windows, so the session ends as it does without them.

Twelve new properties pass (seven layout, four IDML, one separation). An
exhaustive layout property compares 7,704 scope/format/page combinations with an
independent page walk; others cover parent destination pages, threads crossing
sections, kind and section edits with one-step undo, older snapshots, native
spellings through three saves, 17 recovery-only preference variants, guarded
identities after external edits and Preflight errors in both separation paths.
The first test run exposed only test mistakes: an iteration threshold, a page
label expectation, a fixture assigning a parent without its page's `master`, and
two archive expectations that contradicted existing exact-recovery behavior
(native typed definitions are also archived verbatim). Production code did not
change for them.

The source-frozen Windows sweep passes all 16 roadmap targets, shared UI,
formatting and whitespace: **2,105 distinct passing Rust tests** (layout 519,
IDML 364, separation 219, editor 434), four browser checks and eight Python audit
tests; one existing shared UI documentation example remains ignored. Two
macOS-only editor tests are not compiled on Windows, which is why the editor count
is 434 rather than 436. `lint-idml` caught one test-only redundant clone during
the sweep; the corrected file passes `check-idml` and `lint-idml` again, and that
was the only source change. Three files this machine's autocrlf checkout had
written with CRLF were normalized to LF. The variable proof now has 24 pages: its
first 12 are pixel-identical to the previous proof, the 12 parent-page cases match
their literal controls in Poppler and pass visual review, and all 42 other proof
PDFs are byte-identical to `aaad6dc0`. The debug app builds in 12m 20s.

Native review of this build, with Design enabled and isolated configuration,
shows the fixture's native Last Page Number as a typed definition, creates a
document-scope definition rendering "of IV" for labels 1, 2, III, IV, switches it
to section scope ("of 2") and Upper Roman ("of II") through the dropdowns, and
undoes/redoes each edit once. Closing the main window with the variable window
open now ends the process. IDML save/reopen keeps the pages, Roman section and
values; the saved XML uses the published attribute spellings.

Windows notes for the next session: `make`, `zip` and Poppler live in
`C:\Users\Administrator\.schist-tools` (`env.sh` adds them to PATH and sets
`PYTHONUTF8=1`, which the i18n audit needs on Windows). Create validation
worktrees with `git -c core.autocrlf=false` and never share one target directory
between checkouts: Cargo then reuses artifacts built from the other checkout.
Native review runs through the `C:\afprobe` session worker at 800×600.
Next item 9 work: chapter-number variables (document chapter numbering), then
file name, dates and running headers; other recorded item 9 gaps and the INDD
research gate remain. Published to draft PR #195.


Custom-variable authoring, 2026-10-03 (validation stopped for machine handoff):
The Type menu and Stories toolbar open a compact variable manager. New/Edit reveal
two draft fields; Save updates a shared definition once. Insertion and explicit
per-instance removal use a captured Unicode text cursor and preserve coincident
structure order. Pending edits reject stale definitions, stories and document
sessions. Unused definitions can be deleted; referenced definitions require
explicit instance removal first. Typed references in unplaced stories and note
bodies reserve identities. Opaque legacy references remain the codec's concern,
with collision/deletion properties covering repeated native saves. Eleven keys
are in all 150 catalogs. No locale was added.

This entire model/history/UI/interchange batch was implemented before compiling.
The user explicitly stopped validation to move work to another machine. Twelve
make targets passed: `check-layout`, `check-idml`, `check-design`, `check-i18n`,
`lint-layout`, `lint-text-directions`, `lint-idml`, `lint-design`, `lint-all`,
`check-layered-codecs-app`, `check-app-web` and `check-separation`. Completed logs
cover **1,863 distinct Rust tests**, four browser checks and eight Python audits;
this is a partial count, not a completed roadmap sweep. Nine new properties pass
(six layout, three IDML). The first editor compile found a missing closing
delimiter; it was fixed, formatted, and the complete Design rerun passed all 436
editor tests. Workspace clippy and the native app compile check passed.

`check-design-output` was interrupted and must be rerun. Editable-interchange
checks/lint, headless library WASM, shared UI tests, final formatting/whitespace
checks, aggregate proof comparison and the native debug build/window review remain
pending. A formatter run and whitespace check passed before the main rerun; no
further validation ran after the user's stop. The new variable window has not
been reviewed in the native app. The previous full checkpoint (2,086 Rust tests
and native build) belongs to `b0cdef3c`, not this authoring batch.

See [the portable agent handoff](design-handoff.md) for exact resume commands,
current limitations and the copyable prompt. Task temporary logs, proof renders,
research downloads and the isolated Roadmap QA bundle are removed for handoff;
historical `/tmp/schist-*` references below are no longer local artifacts.
Tracked fixtures and the user's development app/configuration are preserved.
Resume the pending checks and UI review, then continue item 9. Non-custom
variables, active initial/nested counts, note-body variables and the other item 9
integration gaps remain; production INDD is still research-gated.



Custom-variable display integration, 2026-10-03:
Main-story literal custom values now use disposable display objects, with source
anchors and shared definitions unchanged. Values stay whole during wrapping,
remain overset when they cannot fit and resume in a wider frame. Object boundaries
isolate bidi/shaping context and source dictionary words; internal spaces do not
expand during paragraph justification. Directional controls add no tracking width.
Variable and note insertions keep source structure order, including coincident
anchors. Unsupported note defaults do not disable independent variables. Effective
paragraph/instance font combinations enter package inventories. No UI or keys change.

Fifteen new properties pass. Six cover layout/source mapping, shared edits, whole
note ordering, overset, empty values and unsupported input; seven cover directional
controls and object semantics across axes, directions, glue, No Break and hyphens.
The IDML property verifies rendering and inherited fonts after repeated saves.
The separation property compares six independent ordinary-text controls in every
process/spot plate through both separation paths at 72/144/216 DPI. All six pairs
in the new 12-page PDF are pixel-identical in extracted samples and Poppler renders;
every page passes visual review. This establishes shared-renderer integration,
not native application placement agreement. All 42 previous PDFs are unchanged.

All 16 roadmap checks, shared UI, formatting and whitespace checks pass:
**2,086 distinct Rust tests**, four browser checks and eight Python audits; one
existing shared UI documentation example remains ignored. The complete layout and
text-engine rerun used the same package set with `--no-fail-fast` to collect errors
in one batch. Initial compile failures identified omitted transient-field defaults.
A mixed-note fixture inherited 13.5-point leading after choosing 14-point type;
correcting its leading resolved the existing overset safeguard without changing
production behavior. Test-only range initialization was corrected for clippy.
Failure logs remain. Only two explanatory comments changed after the main sweep.
Native debug build passes in 5m 38s; the isolated Design-enabled QA bundle is
refreshed and hash/signature verified. Evidence is `/tmp/schist-variable-display-*`.
Five superseded drafts and twelve redundant page renders were removed, retaining
hash audits, the proof PDF/contact sheet and verification evidence. Published as `b0cdef3c` to
draft PR #195; the head, body and draft status were verified without querying CI.

Variable authoring and non-custom evaluation/output remain. Active initial/nested
rules need logical-object counts; those combinations, note-body variables, invalid
anchors, ambiguous/missing definitions and tab/break/control-containing values stay
retained and diagnosed. Other item 9 gaps and the INDD research gate remain. The
next batch should integrate reversible custom-variable authoring using the compact
Design UI patterns, before another compile/test pass.


Native custom text variables, 2026-10-03:
Literal custom definitions now have shared typed data and main-story references.
IDML saves emit native definitions/instances with empty caches and generated IDs;
source text remains unchanged. Guarded document identities and per-story bindings
survive resource reordering and yield to native edits, deletion and class changes.
Unsupported definitions remain exact shared recovery data, and instances remain
unrendered in Schist until composition is connected. No UI or locale keys change.

Thirteen new properties and seven existing variable properties pass. Two failing
regressions reproduced generated IDs activating unrelated unresolved references,
including after an external definition edit. The importer now reserves archived
reference identities by their actual native bindings; missing definitions keep
separate unresolved identities. Legacy recovery IDs keep their original identity.
The suite also covers metadata stripping/corruption, same-name resources, all
source boundaries, inline order, package-wide collisions, source preservation and
one-step edits. A deletion test was corrected to account for the original native
instance remaining present after only its definition was deleted.
IDML clippy passes after correcting a test-only redundant clone. All 16 roadmap
targets, shared UI, formatting and whitespace checks pass: **2,071 distinct Rust
tests**, four browser checks and eight Python audit tests; one existing shared UI
documentation example remains ignored. The source-boundary property covers all
four combinations of story/identity
metadata removal. Both reproduced identity regressions and the deleted-binding
property pass, including legacy recovery instances and three repeated saves.
All 42 PDF proofs remain
byte-identical. Native debug build passes in 20m 23s; the isolated Design-enabled
QA bundle is refreshed and hash/signature verified. Evidence is under
`/tmp/schist-custom-variables-*`; published as `3891674f` to draft PR #195.
The head, description and draft status were verified without querying CI. Cleanup reclaimed
2.3 GiB of superseded test executables, but initially removed a queued executable.
That interrupted run is retained; the complete rerun rebuilt missing artifacts.
A later target-boundary cleanup recovered 2.35 GiB of unused incremental caches,
with no active compiler; builds continue with incremental compilation disabled.
Source code and tests were unchanged during the rerun.

Next is custom-variable projection, atomic fitting and source mapping through
main/footnote flows. Native custom output does not establish application rendering
agreement. Other item 9 gaps and the INDD gate remain. Continue locally without
waiting for CI.



Atomic inline wrapping, 2026-10-03:
TextSpec now accepts transient grapheme-bounded spans which must remain whole
during wrapping. Adjacent spans remain independent, while ordinary boundary rules
and authored No Break still apply. Both shaping paths and discretionary/generated
hyphenation honor the spans; generated hyphen projection remaps their coordinates.
Invalid boundaries or spans containing forced breaks reject composition. Source
edits discard stale spans and serialization does not persist them.

Five properties pass across axes, directions, Unicode boundaries, adjacent spans,
style overrides, generated hyphens and source preservation. Unwrapped ink, bounds
and carets match the ordinary-text controls. Existing No Break and hyphenation
properties pass. All 16 roadmap targets, shared UI, formatting and whitespace
checks pass: **2,058 distinct Rust tests**, four browser checks and eight Python
audit tests; one existing shared UI documentation example remains ignored. All
42 PDF proofs remain byte-identical. Native debug build passes in 7m 34s; the
isolated Design-enabled QA bundle is refreshed and hash/signature verified.
Evidence is under `/tmp/schist-atomic-spans-*`. Published as `63e3c7d5` to draft
PR #195; head, description and draft status were verified without querying CI.

This is wrapping infrastructure for text-variable composition, not live variable
evaluation or native variable output. Those still need typed shared definitions,
instance resolution and integration with the source projection, diagnostics, saves
and note flows. Cached ResultText must not become editable source. No UI or locale
keys change. Other item 9 integration gaps remain; INDD production is spike-gated.



IDML resource identities, 2026-10-03:
A regression reproduced a retained language resource sharing its Self ID with a
generated character style. The exporter now separates part/page-item identity
domains, detects opaque resource collisions against all emitted IDs and remaps
language/list references on a temporary copy. Guarded metadata restores authored
IDs without overwriting native edits or retargeting newly added references. New
language IDs also reserve existing list IDs and unresolved references. No UI or
locale keys change.

Ten properties pass, including three saves of all ten checked-in public IDML
fixtures, referenced object types, source preservation, metadata-free native
references, external edits, malformed/duplicate metadata, unresolved aliases and
page-item IDs near the numeric limit. A failing regression also showed that changing
a resource's native class could restore the wrong saved identity; mandatory resource
class metadata now prevents that restoration. A test type annotation and its expectation
of normalized language-tag casing were corrected; neither required weakening a
production rule. Existing language, list, creation, structured-story and mixed
path-thread checks pass.

All 16 roadmap targets, shared UI, formatting and whitespace checks pass:
**2,053 distinct Rust tests**, four browser checks and eight Python audit tests;
one existing shared UI documentation example remains ignored. All 42 previous
PDF proofs are byte-identical. Native debug build passes in 2m 56s; the isolated
Design-enabled QA bundle is refreshed and hash/signature verified. Evidence is
under `/tmp/schist-idml-identities-*`, including both failing regressions and the
initial successful sweep before the class guard was added. Published as `4cfcac97`
to draft PR #195; head, body and draft status were verified without querying CI. Text-variable
evaluation requires typed definitions, generated-text position mapping and atomic
line fitting; cached ResultText must not become editable source. Other structured
stories and object integration gaps remain open; INDD production is spike-gated.



End Nested Style controls, 2026-10-03:
Main-story ACE 3 markers now stop ordinary nested formatting without adding source
bytes. Explicit single EndNestedStyle bounds, ordered no-style spans and Repeat
consume controls at source grapheme boundaries. Source edits and marker style
renames undo once. Native saves emit markers with their effective formatting;
older recovery-only records upgrade after their native guard agrees, while native
deletions leave archived markers unplaced. The layout kernel consumes typed data.

Nine IDML properties pass, including continuation slices, coincident markers,
paragraph restarts, exact undo/redo, repeated saves and stripped private metadata.
The process/spot proof matches independent explicit ranges through both separation
paths at 72/144/216 dpi. Its split-note case was lengthened after a regression
showed that the first fixture fitted on one frame; it now asserts real continuation.
The new 16-page PDF is integrated into check-design-output. Active initials,
note-body controls and explicit end-marker counts above one remain diagnosed;
native-application agreement is still unverified. None of the ten checked-in IDML
packages contains ACE 3; encoding evidence is the cited firsthand public report.
No UI or locale keys change.

All 16 roadmap targets, shared UI, formatting and whitespace checks pass:
**2,043 distinct Rust tests**, four browser checks and eight Python audit tests;
one existing shared UI documentation example remains ignored. The new eight PDF
pairs are pixel-identical in extracted samples and Poppler renders; all 16 pages
pass visual inspection. All 41 previous proofs are byte-identical. Native debug
build passes in 2m 22s; the isolated Design-enabled QA bundle is refreshed and
hash/signature verified. Sixteen redundant page renders were removed after their
hashes and inspected contact sheets were retained. Evidence is under
`/tmp/schist-end-nested-*`. The original two regressions fail on the preceding
implementation; all nine IDML properties and the new plate property now pass.
Text-variable recovery is published as `0f77ff41` and End Nested Style controls as
`a7c73545` to draft PR #195. Continue locally without waiting for CI. The subsequent
identifier regression and fix are recorded above.
Other structured stories, variables and object integration gaps remain open;
INDD production is still spike-gated.


Text-variable recovery, 2026-10-03:
Seven new properties now preserve native variable instances and their shared
document definitions. The public proof fixture previously lost all three output
date occurrences; the regressions reproduce that loss and now pass. Each instance
retains exact XML, UTF-8 position and effective formatting; eleven native resource
definitions survive byte-for-byte. Shared definitions are stored once per document.
Unknown preferences, malformed metadata and stale native edits remain recoverable,
and live definitions take precedence by opaque identity rather than display name.
Variable content remains explicitly unrendered; evaluation and native output are
still integration work. No UI or locale keys change.

All 16 roadmap targets, shared UI, formatting and whitespace checks pass:
**2,033 distinct Rust tests**, four browser checks and eight Python audit tests;
one existing shared UI documentation example remains ignored. All 41 recorded
prior PDF proofs remain byte-identical. Native debug build passes in 4m 08s;
the isolated Design-enabled QA bundle is refreshed and hash/signature verified.
The focused regression failed before the production change and all seven new
properties now pass. Initial test API typos were corrected before those assertions
ran. Evidence is under `/tmp/schist-text-variables-*`; four superseded drafts and
inventories were removed. Instruction retention is published as `57680ad9` to
draft PR #195. Text-variable recovery is published as `0f77ff41` to the same PR. Continue locally
without waiting for CI, per the user's instruction. Text-variable composition,
other structured stories and the documented object integration gaps remain open;
INDD production is still spike-gated.

Content instruction retention, 2026-10-03:
A regression against the public PSU templates reproduced four page-number
instructions silently disappearing during story decoding. Main Content
instructions now retain their exact bytes, UTF-8 positions, effective named
formatting and native page-number mode as diagnosed recovery data. They add no
source characters. Existing typed footnote markers keep their separate path;
unknown instructions inside opaque containers remain in the original outer XML.
Native marker composition/export remains an explicit gap.

Five new properties and existing footnote/structure checks pass, including
marker-only stories, ordered instructions at every UTF-8 boundary, entities/CDATA,
paragraph breaks, one-step undo, repeated saves and external native edits that
invalidate stale coordinates. Initial synthetic inputs accidentally carried seed
metadata and were corrected to use native story XML; the original fixture failure
and corrected failing-before log remain. The outer-container test distinguishes
exact container bytes from exact PI bytes in constructed context wrappers. Its
original byte comparisons remain in force for whole native containers.

All 16 roadmap targets, shared UI, formatting and whitespace checks pass:
**2,026 distinct Rust tests**, four browser checks and eight Python audit tests;
one existing shared UI documentation example remains ignored. All 41 recorded
prior PDF proofs remain byte-identical. Native debug build passes in 3m 28s;
the isolated Design-enabled QA bundle is refreshed and hash/signature verified.
No UI or locale keys change. Evidence is under `/tmp/schist-story-instructions-*`;
four applied temporary drafts were removed. Prepared for draft PR #195.
Letter-count rules are published as `bc2a19f2` below. Continue the documented
structured-story and object integration gaps locally without waiting for CI.
Sentence segmentation and native control composition still need further evidence;
INDD remains spike-gated.

Letter-count nested rules, 2026-10-03:
Letters now counts Unicode Letter scalars and keeps cuts on whole graphemes.
Numbers, combining marks, punctuation and symbols cannot consume the count.
Ordered no-style spans and repeated sequences share the source cursor. The two
new source-range properties fail on the previous implementation and pass with
the fix. Explicit Unicode examples include Roman numerals, standalone marks,
emoji and a Hangul grapheme containing multiple letters, across every source
continuation slice. Category support reuses an existing transitive dependency.
The policy is documented; complete native Unicode agreement remains unverified.

Four new properties cover source spans, both separation paths and repeated native
saves without generated aliases. All process/spot plates match independent explicit
ranges at three resolutions, including empty spans and actual split notes whose
generated labels contain letters. All eight pairs in the 16-page proof match
extracted samples and Poppler renders, pass visual review, and match final renders.
All 40 recorded prior PDF proofs remain byte-identical.

All 16 roadmap targets, shared UI, formatting and whitespace checks pass:
**2,021 distinct Rust tests**, four browser checks and eight Python audit tests;
one existing shared UI documentation example remains ignored. Native debug build
passes in 5m 08s; the isolated Design-enabled QA bundle is refreshed and
hash/signature verified. No UI or locale keys change. Evidence is under
`/tmp/schist-nested-letters-*`; the two applied temporary Rust drafts and
357 superseded test executables were removed. Old proof-generator executables
were also removed; source, current artifacts, PDFs and verification logs remain.
Prepared for draft PR #195. Repeated sequences are published as `ebcd2d48` below.

Sentence boundaries still need more evidence: native numbering examples contradict
counting every period. Review of the next structural-marker gap found that Content
processing instructions other than typed note markers are lost during decoding.
The existing public PSU fixtures contain four auto-page-number instructions. The
next regression will cover instruction retention, formatting context, source
anchors and existing diagnostics before typed composition is added. INDD remains
spike-gated. Continue locally without waiting for CI, per the user's instruction.

Repeated nested sequences, 2026-10-03:
A bounded no-style Repeat control now loops the requested suffix of supported
ordinary rules. Preceding rules run once; later records stay preserved but inactive.
Cycles restart per source paragraph and stop when a complete cycle cannot advance;
a zero-width member can still be followed by an advancing member. Canonical
initials remain outside the loop. Invalid counts, other control references and
unknown prior bounds produce the existing diagnostics, including after main/note
projection. Public manual/DOM references and a native screenshot establish behavior;
a native Repeat XML record and external placement agreement remain unverified.

Eight new properties verify suffix widths/offsets, skipped spans, every source
continuation slice, Unicode graphemes, no-progress cycles, invalid/ignored records,
initial boundaries, inherited paragraph restarts and edits with exact undo/redo.
Repeated saves preserve source, records, rendered specifications and typed paint.
Every process/spot plate matches explicit ranges at three resolutions in both
separation paths, including generated labels with spaces and actual split notes.
All eight pairs in the 16-page proof match extracted samples and Poppler renders;
every page passes visual review and final renders match the reviewed pixels.
All 39 recorded prior PDF proofs remain byte-identical.

All 16 roadmap targets, shared UI, formatting and whitespace checks pass:
**2,017 distinct Rust tests**, four browser checks and eight Python audit tests;
one existing shared UI documentation example remains ignored. Native debug build
passes in 4m 15s; the isolated Design-enabled QA bundle is refreshed and
hash/signature verified. No UI or locale keys change. Evidence is under
`/tmp/schist-nested-repeat-*`; seven applied temporary Rust drafts were removed.
Prepared for draft PR #195. Word rules are published as `30918024` below.

Item 9 continues with sentence/letter boundaries and the other documented
structured-story/object integration gaps. A further public resource sample adds
literal-space, digit and combined Dropcap/AnyWord records, but no sentence/letter
records. Unicode category support is already a transitive dependency and is being
reviewed for explicit letter classification. INDD remains spike-gated. Validate
locally and continue without waiting for CI, per the user's instruction.

Word-based nested styles, 2026-10-03:
AnyWord now consumes nonempty source words, with through/up-to bounds, ordered
no-style spans, per-paragraph restarts and grapheme-safe continuation slices.
Nonbreaking spaces join terms under the documented bounded Unicode policy.
Generated note labels containing spaces cannot consume source words. LeonidB's
public native sample provides two populated word records; andrejK's screenshots
show native settings in the associated thread. The original stays outside the
repository, with synthetic XML regressions using our own text and metrics. The
original file now imports without the nested-style warning. No external placement
or complete Unicode agreement is claimed.

Six new properties cover source ranges, paragraph breaks, repeated native saves
and process/spot plates in both separation paths at three resolutions. All eight
pairs in the 16-page PDF match extracted samples and Poppler renders; every page
passes visual review and final renders match reviewed pixels. All 38 recorded
prior PDF proofs remain byte-identical. New checks caught invalid fixture offsets
at paragraph separators and stale multibyte endpoints after replacing text; those
fixtures were corrected. Existing unsupported-rule tests now use Sentence because
AnyWord is supported. Proof helper naming/bounds issues were fixed without allows.

All 16 roadmap targets, shared UI, formatting and whitespace checks pass:
**2,009 distinct Rust tests**, four browser checks and eight Python audit tests;
one existing shared UI documentation example remains ignored. Native debug build
passes in 3m 01s; the isolated Design-enabled QA bundle is refreshed and
hash/signature verified. No UI or locale keys change. Local evidence is under
`/tmp/schist-nested-words-*`; prepared for draft PR #195. The earlier delimiter
checkpoint is published as `4c165708`.

This continues Phase 3 item 9. Repeat is next; sentence/letter rules, structural
delimiters, general nested-rule controls and the other documented integration
gaps remain. Public manual/DOM references and a native screenshot establish repeat
semantics, but a populated native Repeat XML record remains unverified. Additional
public XML supplies literal and active initial rules; nine further IDML samples
contain no populated nested lists and were not retained. Production INDD remains
spike-gated. Per the user's instruction, validate locally and continue without
waiting for CI; failures there can be handled later.

Source-derived nested delimiters, 2026-10-03:
Ordered rules compose a supported prefix using AnyCharacter, literal character
sets, ASCII Digits, Tabs, ForcedLineBreak, EmSpace, EnSpace and NonbreakingSpace.
The public user manual defines literal sets as any matching member and digits as
0–9. Through/up-to boundaries, positive repeat counts, no-style spans and a leading
canonical Dropcap share source-derived runs across geometry, typed paint,
dictionary language/No Break and list-marker context. Unknown/invalid bounds
stop the prefix instead of guessing where later rules begin. Original diagnostics
survive main, whole-note and split-note projection, including an unsupported
Repeat whose own style is None after earlier named formatting. Entirely no-style
lists still avoid false errors. Generated labels/aliases never enter saved source.

A regression reproduced an empty source span enlarging and restyling an inserted
footnote number. Consumed rules are now suppressed even when they produce no source
ranges, preserving original direct formatting. Eight layout properties cover
continuation slices, Unicode/grapheme cuts, literal-set order/duplicate invariance,
ASCII versus Unicode digits, missing delimiters and large counts, ordered rules,
paragraph restarts, source edits and exact undo/redo. Two output properties cover
all plates at three resolutions in both paths and retained source diagnostics.
Repeated native saves preserve source, rules, rendered specifications and paint.

All 16 roadmap targets, shared UI, formatting and whitespace checks pass:
2,003 distinct Rust tests, four browser checks and eight Python audit tests;
one existing shared UI documentation example remains ignored. The 24-page proof
matches independently authored ranges in Poppler and every page was visually
reviewed, including vertical text, direct overrides, affine placement, literal
sets/digits, generated labels and actual split notes. Final renders match the
reviewed pixels; all 37 previous PDF proofs remain byte-identical. Native debug
build passes in 4m 17s; the isolated Design-enabled QA bundle is refreshed and
hash/signature verified. Prepared for publication to draft PR #195.

Word/sentence/letter delimiters, Repeat and structural delimiters remain retained
and unsupported. General nested-rule editing is not exposed. Grapheme-safe cuts
follow Schist's Unicode policy; native placement agreement remains unverified.
The next pass has a public native AnyWord example, source hashes and author-posted
screenshots in `/tmp/schist-nested-native-fixtures/`; originals remain outside the
repository. The compact initial-style UI and native count-inheritance fixes are
published as `ea5278f5` and `20f38d17`; their verification is below.

Native initial-count inheritance, 2026-10-03:
A regression reproduced the writer materializing the legacy one-character
initial count on every child of an active style. Reopening then broke future
parent-count edits. The native default now belongs only at the first active
style whose valid ancestor chain cannot supply it. Inactive ancestors and
explicit counts remain unchanged; broken/cyclic chains retain the explicit
fallback. Three properties cover arbitrary-depth/reversed style order, repeated
saves and subsequent parent edits, dormant activation boundaries, explicit zero
resets, and malformed chains. All five focused named-initial tests pass.
All 16 roadmap targets, shared UI, formatting and whitespace checks pass:
1,992 distinct Rust tests, four browser checks and eight Python audit tests;
one existing shared UI documentation example remains ignored. All 37 previous
PDF proofs are byte-identical. Native debug build passes in 2m 46s; the isolated
Design-enabled QA bundle is refreshed and hash/signature verified. No external
application agreement is claimed by these XML/property checks. The prior
`ea5278f5` UI checkpoint is pushed to draft PR #195 and visually verified below.
The `f595465d` desktop run passed Windows/macOS; Ubuntu failed while downloading
the backer catalog with a connection reset, before clippy ran. That failed job
passed on retry. The earlier `942a8137` desktop run passed all three platforms.

Initial character-style authoring, 2026-10-03:
The closed Paragraph / Drop caps disclosure now includes one compact character
style dropdown. Explicit None and named choices preserve other ordered rules;
the separately named Inherit nested styles option restores the complete inherited
list. Count reset retains rules and is now labelled Inherit counts and placement.
Dormant choices stay visible, and missing native references can be replaced
without silently discarding unrelated records. A shared native-record predicate
keeps authoring and composition in agreement while unresolved styles remain
diagnosed. Three short keys and the clarified reset label are in all 150 catalogs.

Three new properties verify captured targets, inherited versus explicit choices,
ordered unknown-rule retention, missing/dormant settings, stale references and
exact undo/redo. All five focused initial-control properties and check-i18n pass.
All 16 roadmap targets, shared UI, formatting and whitespace checks pass:
1,989 distinct Rust tests, four browser checks and eight Python audit tests.
One existing shared UI documentation example remains ignored. All 37 previous
PDF proofs remain byte-identical. The native debug build passes in 3m 54s;
the isolated Design-enabled QA bundle is refreshed and hash/signature verified.
Actual UI review passes on the exact `ea5278f5` CI browser artifact (Web run
37115933543), with Design enabled. The closed default, compact dropdown, reachable
bottom-edge menu, initial-only paint, one-step style/reset undo and redo, None,
inheritance, dormant zero counts and normal-size one-line formatting were checked.
The owned tab, server and downloaded build were removed. Native capture remains
unavailable; the installed app was untouched. The feature flag stays disabled by default.
List-marker/path-initial fixes are published as `f595465d` to draft PR #195,
with their full verification recorded below.

Source list-marker context and path-initial reporting, 2026-10-03:
A regression reproduced a leading footnote reference changing an ordinary black
bullet into cyan superscript. Main/note marker plans now resolve authored context
before reference projection, then map paragraph anchors without changing the
measured marker or source-order counters. Four new properties cover both list
kinds, empty/multiple paragraphs, explicit overrides, named initials, main and
whole/split notes, cross-story chronology, every process/spot plate at three
resolutions and source preservation. The eight-page proof matches independent
explicit marker styling exactly; every page passes visual review. A test fixture
initially used literal newlines instead of paragraph points; the corrected fixture
now exercises distinct paragraph anchors and counters. The split proof was
lengthened after removing enlargement let its shorter note fit without splitting.

A separate regression reproduced Preflight silently accepting enlarged initials
on a text path. Used active multi-line initials now produce the existing localized
unsupported-setting error; zero counts and one-line settings do not. Counts and
source text remain retained. All 16 roadmap targets, shared UI, formatting and
whitespace checks pass: **1,986 distinct Rust tests**, four browser checks and
eight Python audit tests. One existing shared UI documentation example remains
ignored. A fixture-only clippy warning was corrected before the remaining checks
resumed; production code stayed unchanged. The native development build passes in
3m 13s, and the isolated Design-enabled QA bundle is hash/signature verified.
All 36 earlier PDF files remain byte-identical, and final renders match the eight
reviewed pages. Logs, counts and exit codes are under `/tmp/schist-projected-markers-*`.
Three applied/superseded temporary drafts and the older PR input were removed.
No locale keys or default feature flags change.

Named initials are published as `942a8137` to draft PR #195, whose description now
summarizes the accumulated implementation and explicit remaining limits. Its full
local verification is recorded below. Item 9 continues after this checkpoint;
named-initial authoring controls and the other listed integration gaps remain.

Named initial-style composition, 2026-10-03:
A new regression reproduced retained Dropcap rules leaving the source letters
unstyled. A leading canonical native rule now derives character formatting from
the complete source prefix. Font, size, typed paint, dictionary language/No Break,
ordinary list-marker context and font inventories share that resolution. Explicit
source character properties keep precedence. Zero counts disable formatting;
one line applies nominal formatting, while existing enlargement starts at two.
Other nested rules, unresolved references and native placement flags remain
diagnosed. Native application geometry/precedence agreement remains unverified.

Main and note text resolve prefixes before reference projection, using disposable
ranges/aliases only. Nine new properties cover source graphemes, continuation
slices, overrides, writing modes, unknown-rule reporting, dictionary context,
marker context, source edits/one-step undo, native saves and used missing faces.
The 16-page proof matches explicitly authored character ranges exactly in both
separation paths at 72/144/216 dpi and in Poppler; every page passes visual review.
The proof includes affine placement, process/spot inks and actual split notes.
The first sweep was stopped after public-default review identified one-line
named initials exporting no character count. A failing regression confirms the
omission; the writer now emits one for this active legacy-default case, just as
it already did for enlarged initials. Explicit zero and dormant counts remain
unchanged. All 16 roadmap targets, shared UI tests, formatting and whitespace
checks pass: **1,982 distinct Rust tests**, four browser checks and eight Python
audit tests. One existing shared UI documentation example remains ignored. The
native development build passes in 2m 39s; the isolated Design-enabled QA bundle
is hash/signature verified. Logs, counts and exit codes are under
`/tmp/schist-named-initial-*`. Final proof renders match all 16 reviewed pages;
all 35 existing PDF proof files remain byte-identical. Three applied temporary
drafts were removed. No new locale keys or default feature-flag changes are
introduced. The supported subset is documented in
[IDML evidence and limits](idml-format.md#ordered-nested-character-style-rules).

Item 9 continues with list-marker context around generated references: inspection
suggests a leading reference can supply formatting meant to come from authored
text. A regression is drafted but this suspected defect is not yet verified.
Other nested styles, named-initial controls, advanced object/structured-story
behavior, curved frame flow and external application agreement remain open.
Production INDD remains Phase-0-gated.

Source direction is published as `3f1d1ea7` to draft PR #195. Its complete local
verification is below. Three superseded temporary PR descriptions were removed.
Before the new build, 1.86 GiB of closed incremental caches were removed after
checking for active compilers and open files; source and both dev bundles remain.

Source direction around generated references, 2026-10-03:
A regression reproduced a generated Latin reference prefix changing a Hebrew
paragraph's automatic base to left-to-right. Main and note projection now derive
automatic direction from the complete authored paragraph; explicit directions
remain authoritative. Only temporary aliases receive resolved directions, keeping
source text and the saved automatic policy unchanged.

Three focused properties pass: source versus generated direction across scripts,
neutral/empty text and explicit overrides; process/spot plate agreement with
explicit source direction at 72/144/216 dpi in both separation paths; and repeated
native saves without leaked aliases. The eight-page PDF matches explicit-direction
controls exactly in Poppler and every page passes visual review. An initial proof
font lacked decimal digits; its reference style now uses a bundled font covering
both the script and number. All 16 targets, shared UI, formatting and whitespace
pass with **1,973 distinct Rust tests**, four browser and eight Python audit tests.
One existing UI documentation example remains ignored. The 34 existing PDF files
are byte-identical. The native development build passes in 1m 36s. The isolated
Design-enabled QA bundle is refreshed and hash/signature verified; it remains quit.
No new locale keys or default feature-flag changes are introduced. Logs/counts use
`/tmp/schist-projected-direction-*`. Named initial-style composition remains next;
production INDD is still Phase-0-gated.

Source initial counts are published as `fc8c5117` to draft PR #195. Web and
headless CI pass; desktop CI is still running. The preceding compact-control
checkpoint `cb443b27` passes desktop CI on Windows, macOS and Linux.

Source initial counts around generated references, 2026-10-03:
A new regression reproduced a leading footnote number being enlarged while the
first authored grapheme remained body text. Main and note projections now map the
original grapheme prefix into display coordinates. References before or inside
that prefix no longer consume the source count; a reference at its trailing edge
belongs to the body. Only disposable paragraph aliases receive adjusted counts.
Source text, saved styles and native counts remain unchanged.

Three focused properties pass: source ranges/caret boundaries through whole and
split notes, independent process/spot plate agreement at 72/144/216 dpi, and
repeated IDML saves without leaked aliases or counts. The eight-page PDF matches
independently authored prefixes exactly in Poppler; every page was reviewed.
This extends Schist's existing enlarged-prefix policy to generated references;
native flag placement agreement remains unverified and explicitly diagnosed.
All 16 targets, shared UI, formatting and whitespace pass with **1,970 distinct
Rust tests**, four browser and eight Python audit tests. One existing UI
documentation example remains ignored. The 33 existing PDF files are byte-identical.
The native development rebuild passes in 1m 35s. The isolated Design-enabled QA
bundle is refreshed and hash/signature verified; it remains quit. No new locale
keys or default feature-flag changes are introduced. Logs/counts use
`/tmp/schist-projected-initials-*`.

Nested-style retention and native resets are pushed as `ab4022c2` to draft PR
#195. Web and headless CI pass; desktop CI is still running. Its complete local
verification is recorded below.

Ordered native nested-style retention, 2026-10-03:
Public-schema regressions reproduced complete `AllNestedStyles` lists disappearing
and native `EmptyNestedStyles` resets incorrectly inheriting parent rules. The
model and codec now retain ordered typed records, literal versus enumerated
boundaries, signed counts, explicit no-style rules and opaque missing references.
Absent lists inherit; native clear flags and explicit empty lists reset. Local
formatting lowers without changing story bytes. Character-style rename updates
all typed references in one undo step. Malformed/conflicting resets are diagnosed.
The seven Customer's Canvas fixtures verify native clear flags through repeated
saves; populated lists still have only schema-derived coverage.

Rendering of rules requesting character styles remains unsupported and is reported
on import/export and for used paragraphs in Preflight. Entirely no-style lists do
not raise false errors. One short key is present in all 150 catalogs. Twelve new
properties cover retention, resets, Unicode source, rename/undo and used-rule
reporting. All 16 targets, shared UI, formatting and whitespace pass with **1,967
distinct Rust tests**, four browser and eight Python audit tests. One existing UI
documentation example remains ignored. All 33 existing PDF files are byte-identical.
The native development build passes in 1m 21s. The isolated Design-enabled QA
bundle is refreshed and hash/signature verified; it remains quit. The default
Design flag remains false. Logs/counts use `/tmp/schist-nested-*`.

The first native link exhausted disk space; 2.44 GiB of closed incremental caches
were removed, retaining source and both dev bundles. Earlier sweep logs and the
reproduced failures are retained. A subsequent public XML review identified the
native reset flag before publication. Five applied temporary draft files were
removed. Eighteen further public IDML samples contained no active initial/nested
rules; no packages were retained. Research metadata is in
`/tmp/schist-nested-native-research/`. No Adobe headers or INDD entries were read.

Compact initial controls are pushed as `cb443b27` to draft PR #195. The complete
verification and native build are recorded below; exact Web control review passes
in artifact 11267181935 from run 37104336519. The default Design flag remains false.


Compact drop-cap authoring, 2026-10-03:
Paragraph now exposes line/character counts under a closed Drop caps disclosure
with one inheritance-reset icon. Fields capture the original style, validate
integer ranges, and preserve dormant counts, native flags and source text. Blank
restores that count's inheritance; reset clears all three local initial settings
in one edit. Four short keys are in all 150 catalogs. The default Design flag
remains false. Both new count/reset properties and the focused Design target
pass. All 16 targets, shared UI, formatting and whitespace pass with **1,955
distinct Rust tests**, four browser and eight Python audit tests. One existing
UI documentation example remains ignored. All 33 existing PDF proofs are
byte-identical. Logs/counts use `/tmp/schist-initial-controls-*`. The native
development build passes in 2m 28s. The isolated Design-enabled QA bundle is
refreshed and hash/signature verified; it remains quit. Exact `cb443b27` Web artifact 11267181935 passes live Drop caps review:
closed default, compact expansion, both counts, canvas initials, blank-to-inherit,
combined reset and one-step undo/redo. The disposable browser/server and downloaded
build are removed; hashes and results are in `/tmp/schist-initial-browser-*`.

The preceding vertical-initial and command-isolation checkpoint is published as
`f1da870c` to draft PR #195. Its full sweep and native build are recorded below.


Vertical initials and Design command isolation, 2026-10-03:
Physical ink bounds now include upright and sideways vertical glyphs. Initials
compose in both column directions, keep their covered columns together, preserve
source graphemes and retain caret/one-step text undo behavior. This extends
Schist's outline policy; retained native flags remain explicitly unsupported.
An independent plate comparison exposed initial font size changing with frame
position. Paragraph-local scale calculation fixes this for all writing modes;
the translation regression reproduces the defect before the fix.

Design command identity, labels and shortcuts are independent of raster plugins.
Menus and search use the same localized labels; search no longer requires a
hidden raster document for Design actions or offers raster-only actions there.
Raster shortcuts yield to Design alignment and command dispatch. The keymap
property is included in `make check-design` through `schist-app-actions` tests.
No new locale keys or default feature-flag changes are introduced.

The full 16-target sweep, shared UI, formatting and whitespace pass with
**1,953 distinct Rust tests**, including 12 app-actions and five shared UI tests
(one existing UI documentation example ignored). Eight new regression properties
are included; the rest of the count increase brings 11 existing app-actions tests
into the standard sweep. Four browser and eight Python audit checks pass.
The new 16-page proof matches independent ordinary frames exactly in process/spot
plates at 72/144/216 dpi and in Poppler; all pages were visually reviewed. All 32
previous PDF proofs are byte-identical. The first sweep exposed an old test that
assumed horizontal ink offsets even for vertical modes; it now requires physical
ink movement to match the already-verified glyph/caret/selection movement in all
modes. Logs and counts use `/tmp/schist-vertical-initials-*`; earlier failing
checks are retained. The native development rebuild passes in 2m 02s.
The isolated Design-enabled QA bundle is refreshed and hash/signature verified;
it remains quit. Exact `f1da870c` Web artifact 11266834751 passes live command
review with Design enabled and no raster document: localized menus, enabled
search actions, duplicate/delete, selection, alignment and undo/redo. Raster
commands are filtered out; focused numeric input retains Select All, and one
undo restores its committed position. One undo restores a three-object delete.
The disposable tab/server and downloaded build were removed; provenance and
results are in `/tmp/schist-command-browser-{source,qa}.json`. Web and headless CI
pass; desktop CI remains in progress.

Native flag rendering, named initial styles and the recorded structured-story,
object and external-fixture work remain. Production INDD is still Phase-0-gated.


Native drop-cap policy retention, 2026-10-03:
`DropcapDetail` now survives inheritance, explicit zero resets, inactive counts,
local formatting and repeated native saves. Full signed 32-bit values retain
unknown flags without masking. Native count parsing honors the public 0–25 line
and 0–150 character integer ranges; invalid lexical forms and authored counts
are diagnosed rather than truncated or clamped. Public IDML fixtures and legacy
snapshots have coverage alongside local-override and source-preservation rules.
The original horizontal ink/keep regressions remain in place.

This is retention, not native placement. Active explicit flags are reported on
import/export and in Preflight when used; dormant values do not warn. The existing
horizontal outline reservation is unchanged. One short diagnostic key is in all
150 catalogs. All 16 targets, shared UI, formatting and whitespace pass with
**1,934 distinct Rust tests**, including five shared UI checks (one existing
UI documentation example ignored), four browser checks and eight Python audit
tests. Workspace clippy and native/browser/headless checks pass. All 32 PDF
proofs are byte-identical to the preceding reviewed checkpoint. Logs use
`/tmp/schist-dropcap-*`; the native development rebuild passes in 2m 02s.
The isolated Design-enabled QA bundle is hash/signature verified and remains
quit. The checkpoint is prepared for draft PR #195.
Exact `ef572101` Web artifact 11265703900 passes live Hyphenation review with
Design enabled: closed default, clean disclosure expansion, conditional numeric
fields, toggles, reset, numeric editing and one-step undo/redo. All six numeric
fields remain reachable by scrolling. The disposable tab/server and downloaded
artifact were removed. Review exposed raw `edit.delete`/`edit.duplicate` labels
in the Design Edit menu; their actions are routed independently but their labels
incorrectly depend on absent raster registry entries. That UI fix follows this
source-frozen sweep.

Native flag rendering and vertical initials remain the next item 9 work, followed
by the recorded structured-story/object/fixture gaps. Production INDD remains
Phase-0-gated; no INDD bytes or Adobe headers were read.


Automatic dictionary hyphenation, 2026-10-03:
Dictionary opportunities now reach ordinary Design composition through balanced
columns, variable-width frame threads, paths and whole/split footnotes. Complete
source words determine language, protected ranges and word policies before any
inline reference numbers are projected. References never invent dictionary words;
candidates at a coincident reference anchor are deliberately withheld. Trial-owned
histories carry line limits only from accepted lines. Column/frame restrictions
recompose complete words after keeps, with independent note cursors restored when
a frame attempt is rejected. Manual discretionary requests remain independent.

A new regression reproduced an unbreakable suffix painting beyond a narrower
continuation frame after its final dictionary candidate. Word ownership now
survives that last candidate, keeping the suffix overset until it fits. The failing
regression and passing rerun are retained. Initial test failures separately exposed
undersized test frames and incorrect vertical-column detection; those test fixtures
were corrected without weakening the text/history rules.

Paragraph now has a closed Hyphenation disclosure with four icon toggles, numeric
policy shown only while enabled, and a single inheritance reset. Captured edits
undo once, disabling retains dormant settings, and unchanged displayed zones retain
imported precision. Twelve keys are present in all 150 catalogs. The default Design
flag remains false. Pattern coverage and the single-line policy remain explicit in
[Dictionary hyphenation](hyphenation.md); native paragraph-composer equivalence is
not claimed.

All 16 targets, shared UI, formatting and whitespace pass with **1,925 distinct
Rust tests**, including five shared UI tests (one existing documentation example
ignored), four browser checks and eight Python audit tests. Workspace clippy and
native/browser/headless checks pass. All 31 existing PDF proofs are byte-identical.
The new 24-page automatic-hyphen proof matches independently split literal text in
process/spot plates at 72/144/216 dpi and in Poppler; every page was visually
reviewed. Logs use `/tmp/schist-auto-hyphen-*`; the superseded partial sweep is in
`schist-auto-hyphen-initial-sweep`. The native development rebuild passes in
2m 10s. The isolated Design-enabled bundle is hash/signature verified; native
capture still returns `cgWindowNotFound`, so no new control screenshot review is
claimed. The owned QA process was quit, and the installed app remains untouched.
This checkpoint is published as `ef572101` to draft PR #195.
Five superseded integration draft files/directories were removed; source, evidence
and current proof renders remain.

Next is drop-cap native policy and vertical initials, an existing item 9 gap.
Read-only public XML inventory found 20 `DropcapDetail="1"` settings currently
lost by the codec, all with inactive counts. The public specification also bounds
line/character counts that the current reader silently truncates from floats.
No active native drop-cap geometry specimen has been established. Vertical initials
require engine ink geometry: `measure` currently returns no vertical ink bounds,
and the composer explicitly excludes that writing mode. Public source links and
observations are in `/tmp/schist-dropcap-research.json` and
`/tmp/schist-dropcap-next.md`; this was read-only research at that checkpoint. Other
structured-story composition, advanced object styles, curved frame flow, broader
fixtures and external application agreement remain open. Production INDD remains
Phase-0-gated; no INDD bytes or Adobe headers were read in this checkpoint.


Single-line generated-hyphen policy, 2026-10-03:
Dictionary word selection is pushed as `b4abae77` to draft PR #195 with all 16
targets, 1,903 Rust tests and the development build passing. The isolated bundle's
first deep-sign attempt hit a macOS internal error. Signing the flat QA bundle
without `--deep`, then verifying with `--deep --strict`, succeeded; exact notice
bytes and Design flag were also checked. The bundle remains quit.

The text engine now accepts transient generated-hyphen line policy: a consecutive
limit, carried preceding count, non-justified whitespace zone and spacing-versus-
hyphens weight. Manual source SHY retains priority. The zone measures from the
last word, counting trailing separators as whitespace. A documented single-line
raggedness penalty implements weight; no native paragraph-composer equivalence
is claimed. Seven properties pass across all writing axes and paragraph directions,
including monotonic first-line preference, explicit-line reset, source serialization
and variable measures versus correctly carried continuations. Existing generated
and source discretionary tests also pass. All 16 targets pass with **1,910 distinct
Rust tests**, including five shared UI checks (one existing documentation example
ignored), four browser checks and eight Python audit tests. Workspace clippy,
native/browser/headless checks, localization, formatting and whitespace pass.
All 31 PDF proofs remain byte-identical. The native development rebuild passes in
2m 04s; the isolated Design-enabled bundle is refreshed and hash/signature verified,
and remains quit. Logs use `/tmp/schist-line-policy-*`. Ordinary dictionary
composition remains off until frame/column/note trial histories and column-end
restrictions are connected. Review also identified generated footnote digits
changing word segmentation: opportunities must be derived from original source
words before mapping into inline projections. Draft integration notes/code in
`/tmp/schist-line-policy-review.md` and `schist-compose-hyphenation-draft.py` are
unapplied and explicitly incomplete. The dictionary checkpoint's Web/headless
CI pass, and its actual Web artifact contains exact pattern notice bytes; the
61 MB download was removed after comparison.

Disk cleanup removed 655 obsolete test executables and changed-crate library
outputs predating the new dictionary dependency (5.83 GiB logical size). Current
binaries, source, fixtures and PDF proofs were kept; records are in the same log
prefix. Builds still use `CARGO_INCREMENTAL=0`.


Dictionary word selection, 2026-10-03:
Generated hyphen source mapping is pushed as `139e6f03` to draft PR #195, with
all 16 targets, 1,894 Rust tests and the native rebuild passing. Its Web and
headless CI pass; desktop CI remains in progress. The previous discretionary
hyphen checkpoint now has all desktop platforms passing, including Windows.

The next prerequisite selects dictionary opportunities from complete source
words, preserving source coordinates through NFC and length-changing lowercase.
Nine properties cover word limits, explicit language namespaces, mixed runs,
No Break resets, manual hyphens, all source slices, unsupported scripts and long
words. Hypher 0.1.8 is pinned with only reviewed US-English, French and reformed-
German patterns. Unknown regions/orthographies do not fall back silently.
A leading source SHY is also respected: Unicode segmentation drops it from the
word token, which the new regression reproduced before the selector was fixed.
Distribution paths now retain the code/pattern notices; see
[Dictionary hyphenation](hyphenation.md). All 16 targets pass with **1,903 distinct
Rust tests**, including five shared UI checks (one existing documentation example
ignored), four browser checks and eight Python audit tests. Workspace clippy,
native/browser/headless checks, localization, formatting and whitespace pass.
All 31 PDF proofs remain byte-identical. The native development rebuild passes in
1m 43s; the isolated Design-enabled bundle is refreshed with the notices and
hash/signature verified, and remains quit. The macOS app and MCP ZIP paths retain
exact notice bytes in a temporary packaging check using real `ditto` archives;
build/DMG/signing were stubbed, so no release-package validation is claimed.
Shell syntax checks pass. Logs use `/tmp/schist-dictionary-*`; the superseded
partial sweep is retained under `schist-dictionary-initial-sweep`. The word
selector is not wired into composition until ladder/zone/weight and column-end
policies are applied. No new UI or default flag change is claimed.

Disk space fell to about 200 MiB. With no cargo/rustc process running, the unused
incremental cache and superseded release dependency outputs were removed; builds
continue with `CARGO_INCREMENTAL=0`. The built release executable is preserved.
Source files, current binaries, proofs and logs remain.



Generated hyphen source mapping, 2026-10-03:
Native policy retention is pushed as `dee3f6f8` to draft PR #195 with all 16
targets, 1,886 Rust tests and the development rebuild passing. Web and headless
CI pass; desktop CI remains in progress. The next dictionary prerequisite is an
engine path for caller-supplied break opportunities that own no source bytes.
Transient UTF-8 boundaries now produce disposable discretionary glyphs; line
spans, paint ownership, selected tab positions and both caret affinities map back
to the authored text. Selected generated glyphs remain distinct from source
U+00AD and are carried through the layout line painter. Invalid, duplicate,
No Break, whitespace and interior-grapheme positions cannot create extra breaks.
Unwrapped text bypasses the projection. Generated data is not serialized.

Independent visible-glyph and caret-edge properties cover repeated breaks,
Latin/Hebrew word direction, all writing axes, Unicode combining marks and
normal/styled/small-cap/all-cap runs. Review found a real existing defect:
hidden U+00AD split synthetic-small-cap font runs and changed kerning. Invisible
characters now leave itemization intact; selected hyphens keep their own face.
Exact pixel/caret comparisons pass, with a 0.0001 pt tolerance for the existing
legacy-versus-discretionary f32 width accumulation. The first sweep then caught the older Unicode
line-break dependency suppressing Hebrew–SHY–Hebrew. A narrow correction now
permits that intraword case and keeps following joiners/marks and No Break
protected. All 16 targets pass with **1,894 distinct Rust tests**, including five
shared UI checks (one existing documentation example ignored), four browser
checks and eight Python audit tests. Workspace clippy, native/browser/headless
checks, localization, formatting and whitespace pass. All 31 PDF proofs remain
byte-identical. The native development rebuild passes in 1m 57s; the isolated
Design-enabled bundle is refreshed and hash/signature verified, and remains quit.
No new native visual review is claimed. Logs use `/tmp/schist-generated-hyphen-*`.
Dictionary opportunities, policy constraints and authoring controls are not yet
connected, and no dictionary dependency or new UI has been added by this prerequisite.


Native hyphenation policy, 2026-10-03:
XML booleans are pushed as `1a574ac7` to draft PR #195 with all 16 targets,
1,880 Rust tests and the development rebuild passing. The next item 9 regression
confirmed native hyphenation options disappeared on save. The model now retains
all nine independent options, resolves each through style inheritance and keeps
explicit false/zero settings when hyphenation is disabled. Native local overrides
lower once, preserve source text and survive repeated saves. Missing old snapshot
fields inherit; invalid native/authored values are diagnosed. The public weight
range conflict is recorded in `docs/idml-format.md`; 0–100 values retain their
meaning without rescaling. Four codec and two model properties pass. All 16 targets pass with
**1,886 distinct Rust tests**, including five shared UI checks (one existing
documentation example ignored), four browser checks and eight Python audit tests.
Workspace clippy, native/browser/headless checks, localization, formatting and
whitespace pass. All 31 PDF proofs remain byte-identical. The native development
rebuild passes in 2m 02s; the isolated Design-enabled bundle is refreshed and
hash/signature verified, and remains quit. Affinity window inspection also fails
with `cgWindowNotFound`; no test document was opened, and external application
placement remains unverified. Logs use `/tmp/schist-hyphenation-policy-*`.
Dictionary selection and generated-hyphen source/caret mapping remain next.
Public dictionary research records separate language, script and pattern-license
limits in `/tmp/schist-hyphenation-dictionary-research.json`; no new dependency has
been added.


Native XML boolean equivalence, 2026-10-03:
Discretionary hyphens are pushed as `403b44b5` to draft PR #195, with all 16
targets, 1,877 Rust tests, PDF visual review and the development rebuild passing.
The next item 9 package property reproduced numeric booleans dropping explicit
style values. A shared strict xsd:boolean parser now handles all four legal
spellings and XML whitespace across typed document/layer/geometry/style readers.
Existing invalid-value defaults and diagnostics remain; export is canonical and
literal strings/opaque XML are not globally normalized. The new regression covers
both polarities, parent visibility, group/layer/guide locks, frame preferences,
styles, numbering and footnotes through repeated saves. A fixture initially put
its guide beside the page; the coverage assertion caught it, and the fixture now
uses the native page-child location. All 16 targets pass with **1,880 distinct
Rust tests**, including five shared UI checks (one existing documentation example
ignored), four browser checks and eight Python audit tests. Workspace clippy,
native/browser/headless checks, localization, formatting and whitespace pass.
All 31 existing PDF proofs are byte-identical to their prior reviewed versions.
The native development rebuild passes in 1m 25s; the isolated Design-enabled
bundle is refreshed and hash/signature verified, and remains quit. No new native
visual review is claimed. The superseded test draft is removed; logs and research
use `/tmp/schist-xml-booleans-*`. Next is automatic hyphenation: the native policy
is only partly retained, and the existing Hyphenation switch does not yet select
dictionary break opportunities. Language coverage and pattern licenses must be
explicit before adding a dictionary dependency.


Discretionary hyphens, 2026-10-03:
No Break is pushed as `5619c2b9`; its exact Web artifact passes the compact
Paragraph/Character controls review below. The next item 9 property reproduced
invisible U+00AD adding tracking. An initial routing approach also changed legacy
Latin kerning, so the final fix keeps each existing shaper and shares only break
selection. Unused hyphens are invisible and zero-width. Selected hyphens must fit,
paint in their source style and retain UTF-8 caret ranges; No Break and explicit
line ends keep their existing semantics. Box and bounded-path continuations carry
the selected glyph into line painting without editing the source story.
Native literal/numeric XML characters survive repeated saves. Both separation
paths match independent visible/unhyphenated frames in all writing modes and both
reading directions, at three resolutions with process/spot paint. A 24-page paired
PDF proof is wired into the output check. Visual review caught a bad reference:
the terminal literal hyphen and prototype both preceded Latin text in RTL context.
The display glyph now follows the word's resolved direction, with independent
Latin/Hebrew direction controls and hidden Arabic/Devanagari shaping properties.
This is documented typography policy, not a native-application parity claim.
A second regression reproduced source newlines acquiring a hyphen during isolated
line paint. The measuring engine now carries its actual discretionary-break
decision through line spans/composition instead of inferring it from paragraph
position. The corrected full 16-target sweep passes with **1,877 distinct Rust
tests**, including five shared UI checks (one existing UI documentation example
ignored), four browser checks and eight Python audit tests. Workspace clippy,
native/browser/headless checks, localization, formatting and whitespace pass.
All 24 revised PDF pages match independent process/spot controls and pass visual
review, including explicit newlines and both reading directions. The native
development rebuild passes in 2m 08s; the isolated Design-enabled bundle is
refreshed and hash/signature verified. It remains quit, with no new native-window
visual claim. Obsolete test executables reclaimed 903 MB;
current binaries, regression baselines and verification evidence are retained.
Logs use `/tmp/schist-soft-hyphen-*`. Dictionary hyphenation, language-specific
hyphen forms/spelling substitutions and external application placement remain open.
The preceding `5619c2b9` checkpoint now passes Windows/macOS/Linux, Web and headless
CI. Next is the inconsistent handling of numeric/whitespace XML booleans across
native document preferences, layers, geometry and style categories. Public RNC
declares xsd:boolean; the regression will compare complete supported settings
through repeated saves, leaving literal strings and opaque XML untouched.


Native No Break, 2026-10-03:
The clipped-destination checkpoint is pushed as `4640c269`; its Web and headless
CI pass, with desktop CI still running. The next item 9 regression reproduces
NoBreak booleans disappearing during native import/save. Named/local paragraph
and character values now retain inheritance and explicit false resets without
changing source or growing styles on repeated saves. Invalid values are reported;
older snapshots inherit. Enabled cases use the public schema and official
reference; the seven public IDML fixtures contain only explicit false defaults.

Both wrapping paths keep protected ranges together without changing shaping or
caret geometry. A focused property found the whole-story style fast path dropping
the new setting; that path now retains it. Protected lines seek fitting frames or
remain overset, including vertical text. Both separation paths match independent
process/spot frame controls at three resolutions and diagnose terminal overset.
A compact icon toggle and inheritance reset sit in Advanced Typography. Captured
edits undo once. One short key is in all 150 existing catalogs. Unsupported PSD/
Affinity native text export retains its existing private/pixel or reported raster
fallback. Focused engine/layout/IDML/output checks and editor/IDML lints pass.
The final 16-target sweep passes with 1,867 distinct Rust tests, five shared UI
checks included, plus browser/Python audits, formatting and whitespace. All 24 new
PDF pages pass Poppler pair/sample equality and visual review. A spec review
corrected the prototype’s rejection of numeric XML booleans: NoBreak accepts all
four legal literals and surrounding XML whitespace, saving canonically. The
complete sweep includes that correction; its proof PDF is byte-identical to the
visually reviewed artifact. The native development rebuild passes in 2m 09s;
its isolated Design-enabled bundle is hash/signature verified. Native capture
still returns `cgWindowNotFound`. Exact Web CI artifact 11260193813 for `5619c2b9`
passes browser control review with Design enabled: Paragraph and Character both
start collapsed, expansion leaves the document clean, each toggle updates its
active/dirty state, undo/redo restores it, and inheritance reset undoes once.
The disposable tab and server are closed and the downloaded artifact removed;
the installed app was untouched.
Logs use `/tmp/schist-no-break-*`. Dictionary hyphenation, discretionary-hyphen
rendering and external native application placement agreement remain open.


Clipped destination threads and rebuilt popup QA, 2026-10-03:
The explicit-break checkpoint is pushed as `7ec07cf0`. Its exact CI browser
artifact now passes the window-edge review with Design enabled: all seven start
choices fit with the conditional keep counts shown, the bottom even-page choice
commits, one undo/redo restores the previous/new destination, and the combined
inherit reset undoes once. A short style menu and the shared searchable filter
picker also retain their placement and interaction. The disposable browser tab
and server are closed; the downloaded build was removed after recording hashes.
Native CGWindow capture and external native application agreement remain separate
unverified limits.

The next item 9 property reproduced pending page destinations bypassing an
intermediate Clip frame. Skipped frames now honor their terminal overflow policy:
the clipping frame owns the overset text, and its unvisited tail ports remain
empty and addressable. A kernel matrix covers paragraph constraints and explicit
page/odd/even breaks, same-page and later wrong-parity clips, empty/Unicode text,
and source immutability. Both separation paths identify the clipping frame in
Preflight. All 16 roadmap targets pass, with **1,857 distinct passing Rust
tests** (layout 436, separation 198, five shared UI checks included; one existing
UI documentation example ignored), four browser and eight Python audit checks.
Workspace clippy, native/browser/headless checks, localization, formatting and
whitespace pass. The 48-page destination proof is byte-identical to the visually
reviewed artifact. The development build passes in 1m 58s; its isolated
Design-enabled bundle is hash checked and signature verified. Logs use
`/tmp/schist-clipped-destinations-*`; browser evidence uses
`/tmp/schist-parity-breaks-browser-*`. No new user-facing strings.


Explicit numbered breaks and dropdown placement, 2026-10-03:
Native paragraph starts are pushed as `9acfea7e` in draft PR #195. Browser QA of
that exact commit verifies destination edits, one-step undo/redo and the combined
inherit reset, but reproduces clipped last choices when the conditional line
counts push the picker near the window bottom. All editor dropdowns now use
window-constrained anchored popovers, including short inspector menus; the fix
compiles but still needs the rebuilt browser review.

The next item 9 regression reproduces NextOddPage losing its destination during
native import. OddPageBreak/EvenPageBreak now remain distinct in story points,
undo snapshots and native saves. Each pending explicit break records its own
originating page, so consecutive zero-width breaks each advance independently.
Section restarts control odd/even selection. An additional regression exposed
ordinary consecutive column/frame/page breaks before an empty paragraph stopping
in the first frame. Destination instructions now keep that terminal insertion
line flowing even without source bytes; unavailable destinations remain overset.

Four kernel properties and one native repeated-save property pass, covering
leading/consecutive breaks, empty/Unicode text, section restarts, box/path threads,
clipping, protected edits, serialized snapshots, exact undo and balanced whole/
continued footnotes. Independent process/spot plates match ordinary text-frame
controls at three resolutions. Focused layout/IDML/editor lints pass. All 16 roadmap targets pass, with **1,855 distinct passing Rust tests**
(layout 435, IDML 276, separation 197, editor 426, five shared UI checks included;
one existing UI documentation example ignored), four browser and eight Python
audit tests. Workspace clippy, native/browser/headless checks, localization,
formatting and whitespace pass. All 48 proof pages match their controls in Poppler;
the 24 new pages were visually inspected and the unchanged 24 match the previously
reviewed proof pixel-for-pixel. The development build passes in 2m 07s; its isolated Design-enabled bundle was
hash checked and signature verified. CUA still returns cgWindowNotFound. No
document was opened and only the newly launched QA process was terminated.
Rebuilt browser popup verification is pending. Four superseded logs and review
PNGs were removed; regression baselines and reviewed PDFs remain. Logs and QA
records use `/tmp/schist-parity-breaks-*`. No new user-facing strings. External native application
placement agreement remains unverified, and the feature stays disabled by default.


Native paragraph starts, 2026-10-03:
Item 9 now retains all six native StartParagraph choices through inheritance,
local overrides and repeated IDML saves. The compact Keep options group exposes
one destination picker; captured edits preserve unrelated keep policies and undo
once. Its inherit action clears starts and keeps together. Seven short keys are
present in all 150 existing locale catalogs.

Composition separates zero-width start constraints from unconditional breaks.
Already-reached boundaries do not add blank containers. Odd/even choices follow
numbering sections and skip all frames on unsuitable pages. Empty constrained
paragraphs still seek their destination, unavailable destinations retain overset
text, and a satisfied start does not disable final column balancing. Main starts
retain whole/split footnotes, including continuation after main-story EOF; path
threads and mixed writing modes use the same boundary state. Note-body starts
remain retained and preflighted as unsupported. Explicit Br odd/even variants
are still a separate diagnosed gap.

The initial regression failed before native start retention and flow existed.
Boundary tests subsequently found empty paragraphs stopping too early and an
unconsumed, satisfied constraint disabling balancing; both were corrected. One
clipping assertion was wrong because unused tail frames remain addressable, so
it now checks the actual clipping frame. Nine layout, four codec and five captured
keep-control properties pass, as do focused lints and locale checks. Separation
checks cover note-body diagnostics and independently positioned text controls.
All plates agree exactly at 72/144/216 dpi. All 24 PDF pages pass Poppler sample
and rendered-pair equality and visual inspection. This validates shared-renderer
integration, not external native application agreement. All 16 roadmap targets pass, with **1,849 distinct passing Rust tests**
(layout 431, IDML 275, separation 196, editor 426, five shared UI checks included;
one existing UI documentation example ignored). Workspace clippy, native/browser/
headless checks, four browser tests, eight Python audits, localization, formatting
and whitespace pass. The full-sweep PDF is byte-identical to the visually reviewed
artifact. The development build passes in 2m 49s. Its isolated Design-enabled bundle was
hash checked and signature verified, but CUA still returns cgWindowNotFound.
No document was opened, and only the newly launched QA process was terminated.
Browser QA against commit `9acfea7e`'s CI artifact verifies collapsed defaults,
start-choice commits, undo/redo and the single-step combined inherit reset. It
also finds a real dropdown defect: with line counts visible, the menu extends
below the 1690×897 window and clips its final choices. A shared popup placement
fix is required before this control review passes. Five superseded development logs and the
review PNGs were removed; baseline failures, full results and the reviewed PDF
remain. Logs and QA records use `/tmp/schist-paragraph-starts-*`.

The next related interchange gap is explicit odd/even page breaks. Their native
attributes currently collapse to PageBreak with an explicit warning. Further
structured stories, typography, object behavior and external application QA remain
open. Production INDD stays Phase-0-gated and Design Mode disabled by default.


Paragraph keep authoring, 2026-10-02:
The Paragraph panel now exposes native keep policies in a collapsed Keep options
section. Three compact icons toggle line keeps, all lines and keep-with-previous;
first/last counts appear only when relevant. Next-line counts and a single inherit
reset complete the group. Imported or inherited dormant values stay intact.
Eight short keys are present in all 150 existing locale catalogs.

Captured style edits migrate legacy symmetric aliases only when a value actually
changes. Clearing one count then inherits from the base instead of revealing an
old local alias, while unrelated policies remain unchanged. The baseline property
failed before the edit route existed. Four properties now pass across native and
legacy styles, independent flag inheritance, dormant counts, stale targets,
invalid bounds, no-op edits and exact single-step undo/redo. All 16 roadmap targets pass, including workspace clippy and native/browser/headless
checks, with **1,833 distinct passing Rust tests** (425 editor, five shared UI
checks included; one existing UI documentation example ignored), four browser
and eight Python audit checks. Localization, formatting and whitespace pass.
The native development build passes in 2m 41s. Its isolated Design-enabled bundle
was hash checked and signature verified, but CUA still returns cgWindowNotFound;
no document was opened, and only the newly launched QA process was terminated.
Browser QA passes against commit `751d6640`'s CI artifact on an isolated localhost
origin with Design enabled. The group starts collapsed; toggles show conditional
fields, counts commit correctly and survive dormant states, and one undo restores
all reset policies. Invalid next-line input preserves both the prior value and
redo. The panel has no clipping at the tested 1690×897 viewport. This verifies the
actual browser build, not native window access. Logs, counts and QA records use
`/tmp/schist-keep-controls-*`. No new composition or codec behavior
is claimed by this authoring checkpoint.

Native split-note default, 2026-10-02:
Split continuation is pushed as `a0301ac5` in draft PR #195. The public IDML
specification's Appendix C defaults NoSplitting to false, but Schist still rejected
an absent value as unsupported. A new property reproduced that discrepancy.
Composition now resolves absence to the documented split policy while leaving the
authored Option untouched. Explicit true retains the whole-note path. Two new
properties cover exact composition equality across 1–3 independent/spanning
columns and continued native notes through repeated saves with NoSplitting still
absent. Focused tests and layout/IDML lints pass. The full 16-target sweep passes,
with **1,829 distinct passing Rust tests** (layout 422, IDML 271, five shared UI
checks included; one pre-existing UI documentation example ignored), four browser
and eight Python audit checks. Workspace clippy, native/browser/headless checks,
localization, formatting and whitespace pass. The 24-page PDF remains byte-identical
to the visually reviewed continuation proof. The development build passes in
2m 00s, and its isolated Design-enabled bundle was hash checked and signature
verified. The app remains quit. Logs use `/tmp/schist-note-default-*`; bundle
verification is `/tmp/schist-note-default-native-qa.json`. No new UI strings.
Native inspection is still
pending the window-automation issue recorded below. Explicit and omitted-policy
variants of the disposable fixture remain in `target/design-ui/split-notes-qa/`.


Split-footnote continuation, 2026-10-02:
Ruler synchronization is pushed as `21f98d43` in draft PR #195. The next item 9
regressions reproduced long notes being unrendered under NoSplitting=false and
whole-note paragraph keeps losing their reference. An independent cursor per note
now lets main text and note bodies finish separately, including note-only tail
frames. Final overset includes pending note text after main-story EOF. Trial fills
retain their own note and break cursors. First and continued rules, frame policies,
column direction, spanning and final balancing use the same source-safe projection.

Six focused properties pass across Unicode, multiple references, 1–3 columns,
both reading directions, balanced/unbalanced flow, clipping, whole-paragraph keeps,
separator geometry, end placement and explicit main-story frame breaks. The
column property found a second footer being placed in the same frame after main
EOF; the outer loop now advances to the next frame after the split-note pass.
A frame-break assertion incorrectly required text in an already full destination;
it now checks the earliest allowed destination and exactly-once text, accounting
for pending notes. The first complete 16-target sweep passed, including repeated native IDML saves,
both preflight paths and independent plate/PDF controls. All 24 proof pages were
visually inspected. Review then reproduced an affixed reference splitting across
frames while its note had already started. The fit search now evicts an incomplete
generated reference as a unit; the new width/height regression and lint pass.
The repeated full 16-target sweep passes with **1,827 distinct passing Rust tests**
(layout 421, IDML 270, separation 194, plus the unchanged other suites and five
shared UI tests; one existing UI documentation example ignored), four browser and
eight Python audit checks. Workspace clippy, native/browser/headless checks,
localization, formatting and whitespace pass. The final 24-page PDF is byte-identical
to the visually reviewed proof. Final logs and counts use
`/tmp/schist-split-notes-final-*`. The development app build passes in 3m 26s;
its isolated Design-enabled bundle was hash checked and signature verified.
The rebuilt bundle still returns the same `cgWindowNotFound` error, so there is
no native visual result for this checkpoint. Its newly launched QA process was
terminated without opening a document. The small disposable continuation fixture
is retained for resuming that check. Three superseded scratch files and 532 MiB
of unused incremental cache were removed; regression evidence and proof logs remain.

Native QA could not obtain a window from the isolated development app: CUA returned
`cgWindowNotFound` by path and bundle ID, though its inventory showed the process
running. A process sample showed the main thread in its normal AppKit event loop.
No document was opened. Only the newly launched, path-verified QA process was
terminated. This checkpoint has no native visual result yet; the installed app and
public fixtures were untouched. Evidence is `/tmp/schist-split-notes-native-qa.json`.
Current logs use `/tmp/schist-split-notes-*`. No new UI fields or translation keys.


First-paint ruler synchronization, 2026-10-02:
Native paragraph keeps are pushed as `51fd3de3` in draft PR #195. Fresh-launch QA
then reproduced blank Design rulers and a stale 100% zoom label despite a fitted
page. A second observation without input remained stale; clicking the already
active Stories tab restored ticks and the correct 135% zoom. The sample was
closed without changes and the isolated QA app quit.

The pinned GPUI implementation ignores immediate redraw requests during prepaint.
Design canvas preparation now defers the workspace notification until the current
draw finishes after fitting or a bounds change. An initial next-frame callback
attempt compiled but still failed the same native first-open check. This updates ruler extents and zoom chrome
without requiring another gesture or repeatedly repainting an unchanged viewport.
The revised native build passes and its isolated bundle hash/signature were verified.
Fresh-process QA now shows ticks and 135% zoom without another gesture. Native
window zoom and restore update ruler coverage to each canvas extent while retaining
the document zoom. The sample closes without a save prompt; the app was quit.
Evidence is `/tmp/schist-ruler-native-qa.json`. All 16 roadmap targets pass,
with **1,818 distinct passing Rust tests**, including the five shared UI tests
(one existing UI documentation example ignored), four browser and eight Python
audit checks. Workspace clippy, native/app, browser/headless, localization,
formatting, whitespace and independent PDF proofs pass. Logs use
`/tmp/schist-ruler-*`. This adds no user-facing fields or controls.

Native paragraph keeps, 2026-10-02:
Spanning whole notes are pushed as `87a34b2f` in draft PR #195. Three regression
properties first reproduced disabled keeps blocking a single line, native flags
vanishing on save, and keep-with-previous leaving the preceding line stranded.
ParagraphKeeps now retains independent enable/all/first/last/next/previous options,
including inactive values and explicit resets. Legacy symmetric counts/toggles
remain readable and use guarded native metadata for exact authored round trips.
Malformed native changes invalidate that metadata; invalid authored counts are
reported, omitted from native attributes and retained in the guarded label.

Column flow moves the smallest legal complete-line suffix when an adjacent keep
cannot be met. Backwards validation carries heading chains, and frame validation
also handles writing-mode changes. The same pass respects whole-paragraph and
independent widow/orphan limits, opening initials and explicit destination breaks.
A following paragraph shorter than the requested count is kept in full; native
application comparison of this edge case remains needed. No source bytes are
clipped or generated, and layout does not mutate document/history.

The focused tests pass, including all possible split positions, repeated native
saves, independent inheritance, older snapshots and malformed edits. Added
balanced-column/RTL/drop-cap checks also pass in the layout suite. The drop-cap
fixture initially allowed the following line to fit inside its chosen height;
explicit paragraph spacing now isolates the intended rollback case. All 16 roadmap targets pass with **1,813 distinct Rust tests** (layout 415, IDML
269), four browser and eight Python audit checks. Workspace clippy, native/app,
browser/headless, localization, formatting and whitespace checks pass. Full sweep
logs use `/tmp/schist-keeps-sweep-*`. Shared UI adds five passing tests (one
existing documentation example ignored), for **1,818 distinct passing Rust tests**.
The native build passed; `/tmp/schist-keeps-app-build.log` records it.
The previous isolated development build visibly reproduces all three fixture
failures: a disabled keep leaves its first frame empty, a whole paragraph splits,
and numeric keep-with-next moves the whole paragraph and causes terminal overset.
The QA document was closed unchanged and the app quit before rebuilding.
The rebuilt bundle was hash checked and signature verified with Design enabled.
All three cases now paint correctly, and the avoidable terminal overset is gone.
Native 150 dpi PDF export produces a 578 × 758 pt page with complete text and marks;
Poppler visual inspection confirms the corrected flow without clipping or overlap.
Native packaging preserves all six stories and the exact native keep attributes.
Its manifest records Arial Regular, excludes font files and retains font/style
warnings. Packaged IDML reopens with the same corrected flow. Both layouts close
without a save prompt after output/reopen; the QA app was quit. Disposable fixtures,
PDF/package and render images were removed after verification. Evidence is
`/tmp/schist-keeps-native-qa.json`. The installed app and public fixtures were untouched.

Next composition work remains split-note continuation: independent note-body
cursors, continued rules, pending-note overset and trial state through frames,
columns and balancing, including after main-story EOF. Native StartParagraph
choices and external application placement agreement remain separate gaps.

Spanning whole footnotes, 2026-10-02:
Story Editor checkpoint `362c598b` is pushed to draft PR #195. Item 9 now has a
shared footer for explicitly spanning notes, using the full frame width after
insets and reserving room under every column. Height trials use ordinary body
flow and independent break cursors; only the selected layout consumes forced
column/frame/page breaks. Notes are measured once per frame search. Balancing
then minimizes the body height while retaining the reserved footer. Per-frame
and style policies continue to select spanning or independent column areas.
Unknown spanning defaults, split notes, layout-dependent numbering restarts,
vertical/path notes and structured note bodies remain explicit gaps.

Layout regressions cover 2–4 columns, LTR/RTL flow, balanced/unbalanced frames,
terminal and bottom placement, overflow, whole multi-paragraph notes, mixed
frame policies, forced breaks, asymmetric insets, page-relative grids and
impossible fits without orphan references. These tests and layout lint pass.
The independent separation/PDF proof also includes wrapped spanning text crossing
the gutter, with spot/process paints, tint, opacity and affine placement. It
matches independent text/shape frames in every plate at three resolutions and
pixel-for-pixel in Poppler; the 16-page PDF was also visually inspected. Repeated
native IDML saves preserve full-width notes and UTF-8 anchors through document
and frame overrides. The full 16-target sweep passes with **1,803 distinct Rust
tests** (layout 409, IDML 265, separation 192), four browser and eight Python
audit checks. Workspace clippy, native/browser/headless and localization pass.
Logs use `/tmp/schist-spanning-sweep-*`.

The previous native build opens a disposable two-story fixture: independent
column notes paint, while the explicitly spanning notes remain absent. An initial
handwritten fixture omitted its package index; adding that index made it readable.
The installed app and public fixtures remain untouched. Review also found the
layout-open failure branch passing `name` to the existing `{error}` placeholder;
it now uses the correct argument. The affected editor/lint/app/browser checks
and formatting pass again. All five shared UI tests pass (one existing
documentation example is ignored), giving **1,808 distinct passing Rust tests**
with the main sweep. The native debug build passes, and the isolated bundle was
hash checked and signature verified with Design enabled. Its spanning footer now
paints both complete notes across the frame, while the comparison frame retains
independent column notes. Native 150 dpi PDF export produces a 558 × 798 pt page;
Poppler inspection confirms both arrangements, references, rules and printer
marks. Native packaging preserves both source stories, all four note bodies and
their UTF-8 anchors exactly, without persisting generated styles. Its explicit
structured-content/font/style warnings remain visible. Packaged IDML reopens with
the same two arrangements. Both layouts close without a save prompt after output;
the QA app was quit. Disposable fixtures, package, PDF and render images were
removed after verification. Evidence is `/tmp/schist-spanning-native-qa.json`,
and the build log is `/tmp/schist-spanning-app-build.log`.

The next prerequisite identified at the spanning checkpoint was native paragraph
keeps. At that checkpoint, import reduced numeric KeepWithNext to a boolean and
applied KeepFirstLines symmetrically, without honoring KeepLinesTogether,
KeepAllLinesTogether, KeepLastLines or KeepWithPrevious. The public corpus has 20 base styles with keeps disabled but
first/last counts of two, and the academic body style uses KeepWithPrevious=true.
The [public ParagraphStyle DOM](https://developer.adobe.com/indesign/uxp/dom/api/p/paragraph-style/)
and reference manual describe these independent policies. Their model, inheritance,
flow and interchange are addressed in the new handoff entry above. Earlier Schist
keep-rule tests alone did not establish native keep-option fidelity.

Story Editor native window QA, 2026-10-02:
Paragraph balancing is pushed as `e4115390` in draft PR #195. Opening the separate
Story Editor from the Stories panel then reproducibly aborted the native app.
Its constructor and initial synchronous window draw read the workspace while
the originating click still held that entity for an update. The entire window
open is now deferred until the update finishes, with a document-session guard.
The first repaired native build opens successfully; replacing all story text
with one character and undoing once restores the original 425-character story.
Closing the editor leaves the layout open, and closing that layout after undo
does not request saving. This QA also exposed a plural-format call on the
non-plural story-number key and multiline text clipped to one line. The label
now uses its named placeholder, and multiline input measures within its width.
The final native build and isolated bundle hash/signature verification pass.
Two-story native QA confirms wrapped text, active story labels, paragraph breaks,
Unicode, switching without changing the other story, and one-step replacement
undo. PDF export correctly refuses a missing font without leaving a partial
file. An installed-font variant exports both pages as one 876 × 378 pt sheet
at 150 dpi; Poppler renders both balanced pages in order. The native package
contains exact text from both stories, its manifest lists Arial and explicitly
omits font files, and its font/style warnings appear in the window. Its IDML
reopens in Design Mode. All disposable layouts close without a save prompt after
undo/export, and the QA app was quit. The full 16-target sweep passes with
**1,797 distinct Rust tests**, four browser checks and eight Python audit checks.
Workspace clippy, app/browser/headless, localization, formatting and whitespace
checks pass. The shared UI crate also passes all five tests (one pre-existing
documentation example is ignored), for **1,802 distinct passing Rust tests**
across the sweep and that additional crate. Logs use
`/tmp/schist-story-window-sweep-*` and `/tmp/schist-story-window-ui-tests.log`.
Removed 2.3 GB of unused
incremental cache after the drive reached 3.4 GB free; these builds all disable
incremental compilation.
Linux CI on `e4115390` then exposed an incorrect assertion in the paragraph
balancing property: 28 lines across three columns can use the minimum common
height with counts 10/10/8. Minimizing that height does not require equal counts.
The property now checks the maximum column height against the ceiling average
and widow allowance, retaining exact shaped lines, complete consumption, occupied
columns, valid paragraph splits and unchanged source. The affected layout/editor
tests and lints, workspace clippy and formatting pass again; no production layout
code changed for this correction. Remote CI will rerun on the published fix.

Balancing within paragraphs, 2026-10-02:
Checkpoint `f073cc9a` is pushed in draft PR #195. The next regression reproduced
a composition defect: enabling balance on one long, splittable paragraph left
columns empty. Balancing imposed an extra whole-paragraph restriction beyond
the paragraph's own widow and keep settings. Removing that restriction shares
the ordinary paragraph splitting rules, keeping heading chains and grids intact.
A property test covers 2–4 columns, both reading directions, 1–3 widow lines,
one or several paragraphs, first-line indents, unchanged shaped line boundaries
and source preservation. It failed before the fix and passes after it. The full
layout/text suite and the full 16-target sweep pass. A follow-up property also
checks that each complete two-paragraph footnote remains with its reference
when the main paragraph splits across columns. It passes for both reading
directions and preserves all source data. The affected tests/lint also pass after
that addition: **1,797 distinct Rust tests**, four browser checks and eight Python
audit checks. All 16 make targets, formatting and whitespace checks pass; logs
are under `/tmp/schist-paragraph-sweep-*`.
The previous native build also reproduces it at 188%: Columns is 2 and the
balance icon is active, but the single paragraph remains entirely in the left
column. The corrected native build passes, as do the isolated QA bundle's binary
hash and signature checks. At 188%, the same fixture now splits into two columns
with its initial indent preserved. One balance-icon click returns it to sequential
flow and one undo restores the balanced layout. The fixture was then used for
Story Editor checks and closed without saving.


Native column balancing, 2026-10-02:
Frame-footnote checkpoint `27168ad2` is pushed in draft PR #195. Composition
now honors native VerticalBalanceColumns instead of balancing every final
multi-column region. Local true/false values and absence are distinct; enabled
object-style general-frame categories inherit the setting, while disabled
categories do not. Applying or detaching a style preserves the effective policy.
Document creation defaults affect future rectangular frames only. Legacy Schist
snapshots without the field retain their previous automatic balancing; raw
geometry-only composition calls also retain their documented legacy behavior.
The Control panel adds one localized icon beside the column field, with a
selection-wide undo step and predictable mixed-selection behavior. Public
literary XML supplies true frame values and false style/document defaults.
All 16 make targets pass with **1,795 distinct Rust tests**, four browser checks
and eight Python checks. Workspace clippy, localization, native/browser checks,
PDF proofs, formatting and whitespace checks pass. Logs and counts are under
`/tmp/schist-balance-sweep-*`. The native debug build and isolated flagged bundle
hash/signature verification pass. At 188%, one click on the compact Columns
icon switches the handwritten fixture to sequential flow, moving both notes
into the first column. One undo restores the original two columns, references,
note bodies and separate rules. The disposable file was closed without saving;
the installed app and public specimens were untouched.
The existing balancing algorithm still moves whole paragraphs; balancing within
a splittable paragraph is the next composition check after this policy checkpoint.

Frame footnote policies and column flow, 2026-10-02:
Whole-note checkpoint `bb793232` is pushed in draft PR #195. This follow-up
models local frame footnote overrides, their object-style category and inheritance,
and document defaults copied into newly created rectangular frames. Native
TextFrameFootnoteOptionsObject values retain explicit false and dormant spacing.
Applying/removing a style, duplicating a frame and settings edits keep the correct
policy and undo behavior. Existing frames are not rewritten when creation defaults
change. The public academic/literary files provide native document/style records;
active frame policy is covered with explicit native XML and repeated saves.

Whole notes now compose in multiple columns when spanning is explicitly disabled.
Reading direction and forced column breaks select the owning column. Balancing
limits main text height while bottom-aligned notes stay at the physical column
bottom. Spanning areas, split notes and layout-dependent restarts remain pending.
Property tests cover note/reference ownership, source preservation, no overlap,
column order and physical bottom placement. Independent plate comparisons now
exercise one, two and three columns at three resolutions and with rotation.
All 16 make targets pass with **1,788 distinct Rust tests**, plus four browser
and eight Python checks. Workspace clippy, formatting, application checks and the
native debug build pass; logs are under `/tmp/schist-frame-notes-sweep-*`. The
isolated feature-enabled QA bundle matches the built binary before signing and
passes signature verification. Native window access subsequently recovered.
At 100% the actual QA app shows references 7/8 in their respective columns,
with separate rules and note bodies at the physical frame bottom; the local
no-spanning override wins over the document spanning setting. The fixture is
handwritten public-format XML. No external application agreement is claimed.
The first property-test failure was a fixture error: a tall single-column fallback cannot consume multiple
forced column breaks. It now provides real fallback columns instead of weakening
the source-flow rule. External native rendering agreement remains unverified.
Next: preserve the native optional Balance Columns policy; current composition
still balances every final multi-column region automatically.

Whole-note composition, 2026-10-02:
Commit `6c762cfd` is pushed in draft PR #195. All five CI jobs
passed, including Windows, before this follow-up checkpoint.
The next item 9 work composes continuous text-only footnotes in horizontal,
single-column threads with explicit no-splitting enabled. Disposable inline
projections give reference numbers real advances without adding source bytes.
Canvas hits, caret placement and vertical navigation map back to original UTF-8
positions. Note bodies keep independent styles, typed ink paints and markers;
solid separator rules reach preview and print. A monotonically shrinking body
ceiling keeps a reference with its whole note when a frame cannot fit both.
Notes honor inter-note spacing, internal paragraph spacing, first-baseline
leading/ascent and minimum offsets, plus end-of-story placement. Explicit marker
character styles override the document positioning choice. Unknown coordinates,
other numbering/restart policies, multiple columns, splitting, vertical/path
text and unsupported rule/baseline policies still retain explicit diagnostics.
Resource inventories include marker-only inherited font/face combinations.
Projected paragraph text and list-counter outcomes also stay attached to their
lines for preflight; unsupported note tabs and cross-story numbering cannot
silently pass against the main paragraph. Valid local note lists still compose.
Rotated separator rules transform their vectors before antialiasing, matching
independently authored shape output exactly.

The source structure count is now distinct from the composed frame's unsupported
count. Story Editor explains retained content outside its text view; the IDML
retention notice no longer calls every retained item unrendered. Both messages
were updated in all 150 catalogs. All 16 make targets pass with **1,783 distinct
Rust tests** (layout 395, text engine 115, editor 420, IDML 261, separation 191),
four browser checks and eight Python audit checks. Workspace clippy and app,
browser and headless checks pass, as do formatting and whitespace. The independent
text/shape PDF proof passes both plate and Poppler pixel comparisons; upright and
rotated proof pages were also visually inspected. Logs and counts are under
`/tmp/schist-note-flow-sweep-*`. The native debug build passes
(`/tmp/schist-note-flow-app-build.log`). The isolated Design-enabled QA bundle was
refreshed, hash checked and signature verified. Native inspection at 189% confirms
the academic page's superscript reference 4, separator and both note lines,
including the source's literal 5. Subsequent coordinate actions intermittently
returned `noWindowsAvailable` despite a live app and working AX/raised screenshots,
so the extra native typing/undo check was not completed; the editor's source-hit
and navigation property tests pass. The fixture remains unmodified. Design Mode
remains disabled by default.

Next footnote integration work: preserve/apply native per-frame footnote
spacing and spanning overrides, then compose notes with column flow and
balancing. The public PSU files contain TextFrameFootnoteOptionsObject records
with EnableOverrides=false; active overrides are not yet modeled. The document
preferences alone must not be presented as complete native frame-policy support.

Native footnote export and hidden group artwork, 2026-10-02:
Native-window access recovered on the previous build. The public academic IDML
now visibly has the correct red title and concise Layers excerpts. Its black
corner squares exposed group flattening dropping layer membership and visibility.
Children now inherit the nearest explicit layer and cumulative hidden state;
object visibility is separate from opacity, preserved in snapshots/native saves,
and available through compact object eye controls. The academic file retains all
24 hidden parent shapes without sending them to page artwork.

Text-only typed footnotes now export native containers and ACE 4 body markers.
Their original XML remains recoverable. UTF-8 anchors, coincident notes, empty
paragraphs, automatic-direction guards, note-only font combinations and native
text/style edits are covered. A guard flag reads older retention-only saves while
allowing native deletion from newer exports to win. Numbering, note-area
reservation, overflow/splitting and Schist painting remain open. All 16 make targets pass with **1,768 distinct Rust tests** (layout 386, text
engine 115, editor 419, IDML 259, separation 188), four browser and eight Python
audit checks. Workspace clippy, app/browser/headless checks, PDF proofs, formatting
and whitespace pass. Logs/counts are `/tmp/schist-visibility-notes-sweep-*`. The
native debug build passes (`/tmp/schist-visibility-notes-app-build.log`). The
isolated Design-enabled Roadmap QA bundle was refreshed, hash checked and signature
verified. Its academic fixture visibly has the correct red title and no stray
corner artwork. The new object eye hides the title and one undo restores it to
the unmodified state. The installed app remains untouched. Stale release `.rlib`, `.rmeta`,
object and dependency intermediates were removed to reclaim 4.51 GiB; runnable
release/development apps and source evidence remain intact.


Design preview corrections, 2026-10-02:
The native academic PDF comparison exposed red/blue reversal in Design preview.
The pinned GPUI RenderImage contract requires BGRA; Design supplied RGBA for
text rasters, shape fills and placed/warped images. A single upload helper now
converts owned preview pixels at that boundary, leaving source RGBA and native
CMYK used by transforms and separation intact. Opacity variants use the same
helper. A regression covers every alpha value, channel order, source immutability
and the bounded opacity cache.

The compact Layers tree now gives unnamed native `$ID/` objects bounded Unicode
text excerpts or localized kind labels, including parent objects and drag previews.
Saved names remain unchanged. Canvas crop marks now begin after the documented gap.
All 16 make targets pass with **1,760 distinct Rust tests** (layout 384, text
engine 115, editor 419, IDML 253, separation 188), four browser and eight Python
audit checks. Workspace clippy, app/browser/headless compilation, PDF proofs,
formatting and whitespace pass. Evidence is `/tmp/schist-design-preview-sweep-*`.
The native debug build passes; its log is `/tmp/schist-design-preview-app-build.log`.
The isolated Roadmap QA bundle was refreshed, hash checked and signature verified,
retaining the Design flag and separate config/state directories. It launched to
the gallery and opened the native file picker. Final color/label inspection is
pending: automation then returned `cgWindowNotFound` for both QA and Finder.
The QA process remains idle; no new Schist crash report was present. Diagnostic:
`/tmp/schist-final-preview-window-sample.txt`. The installed app was untouched. The preceding typed-body work
is committed as `a4b7e462`; these follow-ups are included in draft PR #195.
Footnote marker composition, note-area reservation,
overflow/splitting and broader native rendering agreement remain open.

Typed footnote bodies, 2026-10-02:
`c131cf14` was pushed to draft PR #195 with preferences and canvas reuse. The
next item 9 step now lowers text-only native notes to independent typed stories
with paragraph/character styles, local overrides and zero-width ACE 4 marker
coordinates. Main-story source bytes and original note XML stay exact. Nested
objects/tables/notes, unknown instructions, mixed content and forced frame breaks
remain opaque. Style renames update note references in one undo transaction;
native style changes used only by a note invalidate stale retention metadata.
Five new properties cover source coordinates, repeated saves, fallbacks, the public
academic body, renaming, parent edits and undo. All 16 make targets pass with
**1,758 distinct Rust tests** (layout 384, text engine 115, editor 417, IDML 253,
separation 188), four browser and eight Python audit checks. Workspace clippy,
app/browser/headless checks, PDF proofs, formatting and whitespace pass. Logs and
counts are `/tmp/schist-footnote-bodies-sweep-*`.
Footnote numbering/marker paint, reserved note areas, overflow and splitting remain
next; typed source data does not yet make notes visible or printable. Native-window access recovered: the `c131cf14` QA build opens the seven-page
academic fixture, switches to compact Layers, selects a frame and shows contextual
Character controls. The first complete post-open screenshot arrived within 46
seconds while the test sweep was running; this is an observation, not a benchmark.
Initial composition remains slow, and imported unnamed objects display `$ID/`.
An idle main-thread sample is `/tmp/schist-academic-cached-ui-sample.txt`. The QA
process was closed after inspection; the installed app was untouched.

Footnote preferences and canvas composition, 2026-10-02:
the prior 1,741-test checkpoint was committed as `39fb6180`, pushed to
`design-tab-leaders` and opened as [draft PR #195](https://github.com/Infrawrench/schist/pull/195).
Further work on this branch adds typed document footnote options with native
Preferences-part interchange: numbering/restarts, affixes, style references,
spacing, baseline policy, splitting/straddling and independent separator rules.
Absent values remain absent; unresolved identities remain explicit. Rule inks and
strokes join exported resources. Settings commits, style renames and swatch edits
retain these references in one undo step. Repeated saves of the public academic
specimen preserve both its preferences and its opaque note payload.
Bodies and reference markers are still opaque and unrendered; space reservation
and typed body/resource lowering remain the next footnote work.

Native-window access worked in this session. The isolated Design-enabled Roadmap
QA app was relaunched and the public academic IDML opened; gallery and the compact
Pages/pasteboard UI were visually inspected. Opening took minutes. A main-thread
sample identifies repeated text composition during canvas planning: the main story
has 13 frames, and each frame recomposed its whole thread. The new kernel pass
reuses ordinary threads and keeps parent results separate by destination page.
The editor retains one complete document/view/font snapshot so unchanged paints
reuse the plan, including correct invalidation for IME drafts and undo. Kernel
cache properties and editor checks pass. The native debug build also passes and
the isolated QA bundle was refreshed, hash checked and its signature verified.
It retains the Design flag and separate config/state directories. The new process
launches, but native automation returns `cgWindowNotFound`; its main thread is
idle in the event loop. Updated visual/performance verification remains open.
The older QA process was closed. Samples: `/tmp/schist-academic-ui-sample.txt`
and `/tmp/schist-roadmap-launch-sample.txt`.

All 16 make targets pass with **1,753 distinct Rust tests** (layout 382, text
engine 115, editor 417, IDML 250, separation 188), four browser checks and eight
Python i18n-audit checks. The 12 new properties cover preferences/interchange,
style renaming, immutable thread reuse and complete canvas-cache invalidation;
the existing swatch property also exercises both footnote rules. Workspace clippy,
native/browser/headless checks, PDF proofs, formatting and whitespace checks pass.
Results/counts/logs are `/tmp/schist-footnotes-canvas-sweep-*`; the native build log
is `/tmp/schist-footnotes-canvas-app-build.log`. This follow-up is part of draft
PR #195. All five remote checks, including Windows, passed on the earlier
`39fb6180` checkpoint; the follow-up must receive its own CI result.
Unused incremental build cache was removed after checking that no incremental
compiler was using it. Design Mode remains disabled by default.

Structured-story retention, 2026-10-02: imported tables, footnotes and inline
page items now retain exact outer XML rather than disappearing after a warning.
UTF-8 anchors survive nearby edits; crossing edits are refused. Snapshots and
paragraph/character styling preserve data through one undo step. Threading refuses
to replace opaque-only stories. Guarded standard Story Labels retain Schist data
through saves; native body/format/style-name edits win and leave retained payloads
with unknown locations. Malformed or duplicate metadata stays recoverable.

The public PSU table, footnote and inline math payloads survive four saves exactly.
Story Editor shows a compact retained count; both preflight paths flag missing
structure paint even for empty/fitting body text. Three new keys and the corrected
existing warning are in all 150 catalogs. This is preservation, not native
structured composition or reconstruction of referenced resource graphs.

The full sweep passes all 16 make targets with **1,741 distinct Rust tests**
(layout 377, text engine 115, editor 415, IDML 245, separation 188), four browser
checks and eight Python i18n-audit checks. The 12 new properties cover preservation,
UTF-8 edits, styling/undo, external changes and both print paths. Workspace clippy,
native/browser/headless checks, existing PDF proofs, formatting and whitespace
checks pass. Logs/results/counts are `/tmp/schist-story-structures-sweep-*`.
The native debug build passes; its log is
`/tmp/schist-story-structures-app-build.log`. The isolated Roadmap QA app bundle
has been refreshed from this executable and its signature verified, retaining the
Design flag and separate config/state directories. It has not been relaunched or
visually inspected; an already-running QA process still uses its previous binary.
Unused incremental cache and superseded scratch output were removed; current
verification logs and public reference evidence remain.

Public footnote input/PDF evidence is saved at
`/tmp/schist-native-footnote-reference/` and described in `docs/idml-format.md`.
Its visible output differs from several fixture comments, so those comments are
not accepted as native placement/default evidence. Typed footnote preferences,
body/marker composition and space reservation remain the next structured-story
work. Item 9 remains active; native-window QA and production INDD remain gated.
This checkpoint is prepared for draft review on `design-tab-leaders`, with
Design Mode disabled by default.

Same-page cross-story numbering, 2026-10-02: authored object chronology is now
independent of paint order and story indices, recorded in the creation gesture's
undo transaction, and retained in guarded standard object Labels. Unlabelled native
imports remain unknown; ambiguous or changed metadata is rejected rather than
manufacturing evidence. Deletion retains chronology for undo and excludes unplaced
story tombstones from the live list.

One ordinary unthreaded frame per used story on one page now composes a shared
sequence in known creation order. Multilevel ancestor events remain monotonic across
story byte-offset resets. Independent lists, explicit restarts and source bytes
are preserved. The existing Paragraph list disclosure selects a sequence and
changes its shared continuation setting in one undo step. Supported continuation
has a Schist-order notice; unknown chronology, threaded/parent/multiple-page/book
sequences are still diagnosed without guessed markers. All new keys are in the
150 catalogs. Native rendering parity is not claimed.

The full sweep passes all 16 make targets with **1,729 distinct Rust tests**
(layout 372, text engine 115, editor 415, IDML 239, separation 187), four browser
checks and eight Python i18n-audit checks. Workspace clippy, native/browser/headless
checks and output proofs pass. Print plates match independent per-story references
exactly at 72/144/216 dpi; both preflight paths expose unknown order. Frame,
thread and chronology indexes are call-local so each participant does not rescan
the whole document. Logs/results/counts are `/tmp/schist-cross-story-sweep-*`.
The new English notice has an exact deferral under the existing disabled-feature
translation rule. Evidence and limits are in `docs/idml-format.md`.
The native debug build passes (`/tmp/schist-cross-story-app-build.log`). Obsolete
incremental cache and superseded app/editor build files were removed after it
nearly exhausted disk space. Item 9 and native-window/INDD validation remain open. At this checkpoint, review
found table/footnote nodes were omitted despite the model's preservation comment.
The later structured-story checkpoint above fixes that data loss.


Native hanging-indent tab integration, 2026-10-02: public InDesign output now
pins 11 marker/tab placements and an ordinary source-tab placement. A virtual
hanging-indent stop precedes a later explicit stop or the implicit grid. The
previous c07 marker body landed at 60 instead of 30; ordinary c12 landed at 36
instead of 40. Regressions are retained in
`/tmp/schist-native-marker-tabs-before.log` and
`/tmp/schist-native-source-indent-before.log`. Geometry and leader ownership now
agree with those observations. Native font/raster or other-axis agreement is
not claimed; provenance and limits are in `docs/idml-format.md`.

The shared source-tab geometry keeps the indent column-relative through wrapping,
paint, carets and scaling. A virtual stop cannot borrow an explicit leader.
Properties cover passed stops, explicit collisions, ruler order and inherited
indents; four native saves retain masks, source bytes and carets in both directions
and all three axes. The source-tab fallback notice is corrected in all 150
catalogs and its explicit English deferral. The 202-case tab proof and 36-case list
proof still pass exact plates at three resolutions and every paired PDF sample.

The save property exposed another real loss: paragraph-local writing modes were
omitted from IDML. Standard Label metadata now retains explicit Horizontal,
VerticalRightToLeft and VerticalLeftToRight overrides, preserving inheritance
when unset. Import/export report that this is Schist-only orientation metadata;
no native paragraph attribute or mixed-axis rendering agreement is invented.
A new notice is present in all 150 catalogs. Misleading align documentation now
states its deliberate per-object undo exception, and duplicated comment text is
removed. Undo behavior is unchanged.

All 16 make targets pass with **1,716 distinct Rust tests** (layout 366, text engine
115, editor 414, IDML 234, separation 186), four browser checks and eight Python
i18n-audit tests. Workspace clippy, native/browser/headless checks, output proofs,
formatting and whitespace pass. Logs/results/counts use
`/tmp/schist-native-marker-tabs-sweep-*`. The native debug build passes; its
log is `/tmp/schist-native-marker-tabs-app-build.log`. An isolated Roadmap QA
bundle was prepared from this binary with the Design flag enabled; existing dev
and installed app bundles are untouched.

The public cross-story numbering PDF was inspected completely: both pages have
1/2 in story A and 1 in story B, despite a shared continuation input. The pages
are pixel-identical, so their labels do not establish restart semantics. Adobe's
public guide specifies frame-creation order for unthreaded frames on one page;
story-vector and paint order are not safe substitutes. Item 9 continues with the
ordering model and remaining advanced text/interchange work. Production INDD
remains spike-gated; native-window QA is still unavailable. Work is uncommitted
on `design-tab-leaders`, and the feature default remains false.


Initial-tab fallback and horizontal marker leaders, 2026-10-02: diagnosed source
tabs inside an enlarged initial now preserve ordinary source flow, wrapping,
pixels and carets. The prior composition still enlarged that unsupported tab gap;
the regression is `/tmp/schist-initial-tabs-before.log`. Native reservation and
scaling behavior remains unverified, so the retained setting stays diagnosed.

Generated bullet/number tabs can paint literal leaders from their selected
explicit leading stop. Passed/implicit stops and legacy fixed gaps cannot borrow
one. A counter has separate marker/leader paint fragments, preserving original
marker pixels, counter strings and all source carets. The independent proof
exposed fractional marker-frame rounding; the leader fragment now uses the column
ruler independently of marker placement. Non-leading, RTL/vertical/path/initial
marker combinations remain open and diagnosed. Four native saves retain strings,
styles, counters and paint. No native leader phase agreement is claimed.

The list proof now has **36 cases / 72 paired pages**, including 12 new leader
cases. Exact plates agree at three resolutions, and Poppler compares every paired
page/sample. Every new actual case passed visual review; evidence is
`/tmp/schist-leaders-visual/marker-contact.png`. All 16 make targets pass with
**1,712 distinct Rust tests** (layout 364, text engine 114, editor 414, IDML 233,
separation 186), four browser checks and eight Python i18n-audit tests. Workspace
clippy, native/browser/headless checks, PDF output, formatting and whitespace pass.
Logs/results/counts use `/tmp/schist-marker-leaders-sweep-*`. Native debug build
passes (`/tmp/schist-marker-leaders-app-build.log`); the open isolated dev bundle
has not been replaced, and native window QA remains unavailable.

New public native list-marker PDFs and fixture inputs, pinned to the same paged-media
revision as the source-tab reference, are in `/tmp/schist-native-list-reference/`.
Only fixture definitions and public PDF output were consulted. `list-markers.pdf`
shows a virtual hanging-indent stop before a later explicit stop (case c07:
left indent 30, explicit stop 60, body at 30). Marker composition at that checkpoint instead
chose 60; the later native hanging-indent checkpoint above corrects this geometry
and leader ownership. Remaining item 9 work continues. The public cross-story numbering fixture is
also available; its page label says restart, but the fixture defines continuation
for both pages, so the label is not evidence of reset behavior.

Production INDD remains spike-gated. Changes are uncommitted on
`design-tab-leaders`, and Design Mode remains disabled by default.


Horizontal path/tab integration, 2026-10-02: source tabs now use the path
bracket's logical arc-distance ruler, independently of page coordinates and
first-line/hanging indents. The general anchor property covers both directions,
all four alignments and multiple indents; it failed before the origin fix.
Straight, rotated and cubic baselines paint ordinary fields and literal leaders.
Vertical path tabs and the other unimplemented paragraph/initial combinations
remain diagnosed.

The independent proof has **202 cases / 404 paired pages**, including 64 path
cases. Exact plates agree at 72/144/216 dpi; Poppler comparisons pass and all
64 actual cases passed visual review. The center-aligned group's 16 PNG pages
are pixel-identical to the already reviewed leading-aligned group. Evidence is
`/tmp/schist-leaders-visual/path-contact-*` and `path-page-*`. This validates
Schist integration, not native curved-tab rendering agreement.

Curves exposed accumulated shaping error and differing glyph fill/stroke
projections. Advances now accumulate before f32 coordinate rounding. Both paints
share a 1/64-pixel inline sampling grid, with unsnapped document geometry and
carets. Four native saves retain source text, rulers, cubic handles, brackets,
pixels and carets. Recomputed cubic bounds also shifted the local origin;
standard guarded Label metadata now retains authored local bounds only while
native geometry agrees. External curve edits supersede it, and native transforms
continue to apply. No native geometry or rendering semantics are inferred from
this precision metadata. Properties verify the guard, continuous carets and
bounded sampling separately from the output proof.

All 16 make targets pass with **1,710 distinct Rust tests** (layout 362, text
engine 114, editor 414, IDML 233, separation 186), four browser checks and eight
Python i18n-audit tests. Workspace clippy, native/browser/headless app checks,
PDF output, formatting and whitespace checks pass. Logs/results/counts use
`/tmp/schist-path-tabs-sweep-*`. The native debug build passes; its log is
`/tmp/schist-path-tabs-app-build.log`. Native window QA still returns
`cgWindowNotFound` for `com.infrawrench.schist.dev`; no claim is made that the
open isolated dev bundle contains this checkpoint's binary.

Next item 9 work is initial/source-tab behavior and generated marker tabs, then
cross-story/further-format lists, dictionary hyphenation, vertical initials,
structured stories, alternate layouts, advanced objects and further native
validation. Production INDD remains spike-gated. Changes are uncommitted on
`design-tab-leaders`, and the feature default remains false.


Native source-tab collision/edge integration, 2026-10-02: a public InDesign
20.0.1.32 PDF and its fixture inputs establish horizontal LTR collision and
beyond-frame wrapping behavior. Ahead-of-pen aligned stops clamp a field to the
pen; only passed stops are skipped. The old collision-skip policy was incorrect.
All 52 native sweep observations now match the numeric geometry within PDF
bearing/advance tolerance. Provenance, the pinned revision and PDF hash are in
`docs/idml-format.md`; only public fixture definitions and output were consulted.
The failing original rule is recorded in `/tmp/schist-native-tab-collision-before.log`.

Zero-advance tabs retain source bytes, caret positions and their selected stop,
with no leader ink. A terminal tab after text can end at the line's inline edge,
letting its following field wrap. The same measure reaches standalone paint,
carets and zoom through `TabStops.line_width`. Leading-tab overset still resumes
unchanged in a wider frame. Properties cover all axes, explicit directions,
origins, widths, first-line indents and source coverage. Implicit intervals that
cannot advance at f32 precision fail rather than pretending to be zero-gap tabs.

The independent print proof has **138 cases / 276 paired pages**, with 18 new
touching-field cases. Exact plates pass at 72/144/216 dpi, and all new actual pages
passed visual review. Evidence is `/tmp/schist-leaders-visual/collision-contact-*.png`.
All 16 make targets pass with **1,706 distinct Rust tests** (layout 361, text engine
112, editor 414, IDML 232, separation 186), four browser checks and eight Python
i18n-audit tests. Workspace clippy, app/native/browser/headless, output, formatting
and whitespace checks pass; logs/results/counts use `/tmp/schist-native-tabs-sweep-*`.

Native Schist window QA still returns `cgWindowNotFound`. The new native debug
build passes (`/tmp/schist-native-tabs-app-build.log`); it does not replace the
open isolated dev bundle. Other
paragraph alignment, justified aligned tabs, path/initial-tab combinations,
generated marker tabs and the subsequent item 9 gaps remain open. The native PDF
settles the observed LTR geometry only, not full native font/raster agreement or
RTL/vertical behavior. Work remains uncommitted on `design-tab-leaders`; the
feature default remains false. Nine superseded task temporary files were removed.

RTL source-tab integration, 2026-10-02: right-aligned horizontal RTL paragraphs
now use a right-edge column ruler and first-line/hanging indent; vertical RTL
keeps downward inline progression. Native LeftAlign/RightAlign stop names map to
physical field edges after resolving the complete paragraph's direction. Character
anchors convert their physical caret to ruler distance. Column starts, wrapping,
threading and standalone line rendering share that origin, including automatic
direction and story-inherited vertical axes. Four repeated native saves preserve
stop names, styles, text, raster masks and carets.

The independent print proof now has 120 cases (240 paired pages), including 48
explicit/automatic RTL cases with real Hebrew glyphs and separately styled numeric
fields. All cases fit one line, exact plates match at three resolutions, and all
48 new actual cases passed visual review. It found a real mirroring cancellation
bug at zero; using the stored field end fixes the shifted glyph mask. The regression
is `/tmp/schist-rtl-before-paint-fix.log`; the earlier character-anchor failure is
`/tmp/schist-rtl-before-fix.log`. Exact final-field masks are also checked over seven
sizes, five scales and every stop alignment.

Generated RTL list markers still require their own placement/composition. They now
remain unpainted with an explicit diagnostic in IDML and both preflight paths,
instead of using the wrong column edge. Continued-line diagnostics retain the whole
paragraph's direction. No native RTL reference fixture establishes InDesign
agreement; ruler interpretation is recorded with public sources and identified
inferences in `docs/idml-format.md`. Other paragraph alignment, justified aligned
source tabs, path/initial-tab combinations and generated marker tabs remain next.
All 16 verification targets pass: **1,703 distinct Rust tests** (layout 360,
text engine 110, editor 414, IDML 232, separation 186), four browser checks and
eight Python i18n-audit tests. Workspace clippy, app/native/browser/headless,
i18n, output proofs, formatting and whitespace checks pass. Logs/results/counts
are under `/tmp/schist-rtl-sweep-*`. Native window QA remains unavailable because
computer-use cannot locate the isolated development window.

Tab leaders, 2026-10-02: Phase 3 item 9 now paints literal leader units for
ordinary source tabs in all four stop alignments and three writing modes. The
selected explicit stop owns its leader; skipped and implicit stops cannot borrow
one. Repeated units inherit the source tab's resolved font, ligatures, capitalization,
paint and baseline offset without adding story bytes or caret positions. The
folded Tabs section edits the selected stop's literal leader in one undo step,
including inherited records, clearing and stale-target rejection. Two short
labels are present in all 150 catalogs.

The repetition policy fits complete shaped units against the following field's
edge, leaving spare advance beside preceding text. Native repetition phase remains
unverified. A property over 1–128 units found a lost final unit from f32 scaling;
the quotient now snaps within arithmetic precision while real partial units remain
partial. The failing evidence is `/tmp/schist-leaders-rounding-regression.log`.
Leader enumeration is bounded at paint time and never performed during wrapping.
Empty/nonpositive-width units produce no repeated ink. Extreme finite coordinates
and oversized text bitmaps fail rendering safely; both separation paths now report
failed text as an error rather than silently omitting it.

All 16 verification targets pass with **1,696 distinct Rust tests** (layout 357,
text engine 108, editor 414, IDML 231 and separation 185), four browser checks and
eight Python i18n-audit tests. Workspace clippy, native/browser/headless app checks,
formatting and whitespace checks pass. The tab proof now has 72 cases and 144 paired
pages, compared at 72/144/216 dpi; all 24 new actual leader cases passed contact-sheet
review. Its independent ordinary-text reference uses zero-width-space tracking for
fractional placement within an integer-positioned frame. Logs/results/counts use
`/tmp/schist-leaders-sweep-*`; visual evidence is `/tmp/schist-leaders-visual/`.
`CARGO_INCREMENTAL=0 make build PROFILE=debug` passes. Native window QA of this
leader field remains pending: the computer-use API currently returns
`cgWindowNotFound` for the running isolated Schist Dev app.

Work continues with RTL/paragraph alignment, justified aligned tabs, path/initial-tab
cases and generated list-marker tabs, then the remaining item 9 fidelity gaps below.
Native InDesign agreement remains unverified and the feature default remains false.
The leader changes are uncommitted on `design-tab-leaders`, based on merged PR #194.

Aligned paragraph tabs, 2026-10-01: Phase 3 item 9 now composes right, center
and character/decimal source tabs using the following field's shaped metrics.
Caret positions, mixed styles, ligatures, all three writing modes, column origins,
indents, wrapping and threading share those anchors. Preview inspection also
found that the pasteboard scaled glyphs without scaling tab rulers; the ruler
and its origin now scale together, with caret-position properties across zooms. Empty fields still advance.
Repeated IDML saves preserve inherited/replaced/cleared stops and literal
alignment characters. The folded Paragraph Tabs section edits one selected stop
with alignment icons; inherited edits, deletion and restoring inheritance are
single undo operations, and stale field targets cannot overwrite changed records.
Five new short labels are in all 150 catalogs.

The 48-case print proof compares tabbed text with independent frame placements
at three output resolutions. An overlapping reference case was moved to avoid
collision; later native evidence showed that the engine's collision-skip rule
was wrong, and the subsequent native-tab checkpoint corrects it. The proof also
found a separate integer pixel
rounding defect. Glyph fill and stroke placement now ignore f32 round-off near
integer boundaries without changing document coordinates. The corrected proof
fails without that fix and passes with it; see `/tmp/schist-aligned-tabs-rounding-regression.log`.
The PDF proof now has 96 pages; all pairs match and all 48 actual cases passed
visual review. All 16 verification targets pass: **1,687 distinct Rust tests**
(including layout 357, text engine 102, editor 413 and IDML 231), four browser checks and eight
Python i18n-audit tests. Workspace clippy, native/browser/headless app checks,
formatting and whitespace checks pass. Logs/results/counts are under
`/tmp/schist-aligned-tabs-sweep-*`. `CARGO_INCREMENTAL=0 make build PROFILE=debug`
passes. The resulting executable matches `target/design-ui/Schist Dev.app`,
running with `design-mode=true` and isolated preferences. Native window checks
verified right/center/decimal alignment, position and character edits, add/remove,
inheritance and one-step undo. The preview's 135% zoom now preserves the ruler's
position. The saved `target/design-ui/aligned-tabs.idml` demo is open with its
compact Tabs section visible; its native XML contains the selected character
alignment, literal period and 220 pt position. App/build logs use the same
`/tmp/schist-aligned-tabs-*` prefix.

At the aligned-tab checkpoint, next item 9 work was tab leaders, then native RTL/paragraph-alignment,
justified aligned tabs, path/initial-tab cases and generated list-marker tabs.
Cross-story/further-format lists, dictionary hyphenation, vertical initials,
structured stories, alternate layouts and advanced object behavior remain open.
Collision and absent-character fallbacks are documented Schist policies, pending
native reference fixtures. Unsupported combinations remain retained and diagnosed.
Native application agreement is unverified; INDD remains Phase-0-gated and the
feature default remains false.

The compact UI changes were merged in
[PR #193](https://github.com/Infrawrench/schist/pull/193) as `ebe9e82c`. All its
hosted checks passed, including Windows, macOS, Linux, web and headless builds.
The aligned-tab follow-up merged in [PR #194](https://github.com/Infrawrench/schist/pull/194)
as `edfbebcd`; all five hosted checks passed, including Windows, macOS, Linux,
web and headless builds.

Compact Design panel contents, 2026-10-01: Layers now has a collapsible tree,
visibility/lock columns, object-type icons and full-name tooltips. Dragging layer
names inserts them in order; dragging selected object names onto another layer
moves the selection in one undo step. Shift-click selects multiple rows. Model
operations preserve other layers' relative order and implicit first-layer
membership, reject invalid or locked object drops atomically, and omit no-op
history entries. Object stacking within a layer is unchanged.

Pages, Links, Stories and Preflight use compact icon toolbars; filenames and
story previews fit their rows. Swatches have readable list rows with colour
inputs behind Appearance. Character formatting and paragraph alignment have
active-state icons. All action tooltips reuse existing localized keys; ten new
SVG icons are registered for native and browser builds. Native inspection in
`target/design-ui/Schist Dev.app` with `design-mode=true` verified layer/object
drags, one-step undo, disclosure, visibility, locking, formatting, swatches,
story previews and preflight. The scratch document was restored after edits.

All 16 verification targets pass: **1,675 distinct Rust tests**, including all
410 editor tests and five new layer-operation properties, plus four browser
checks and eight Python i18n-audit tests. Workspace clippy with warnings denied,
native/browser/headless checks, PDF proofs, formatting and whitespace checks
pass. Logs/results/counts are `/tmp/schist-design-compact-sweep-*`. Changes are
on `more-indesign`; the default feature flag and remaining item 9 gaps are
unchanged.

Design workspace UI follow-up, 2026-10-01: the always-expanded stack is replaced
by a single active panel, related tabs and a collapsible icon rail. Page or object
geometry lives in a compact top control bar; advanced type, paint, list, page and
object-style settings expand on demand. Saved workspaces retain every Design
panel key, visibility, active panel and collapse state. Eight new labels are in
all 150 catalogs, and Tracking now shows its actual thousandths-of-em unit.

Native UI inspection uses the local `target/design-ui/Schist Dev.app`, built from
this checkout with `design-mode=true` and isolated preferences. It verified the
Pages/Character/Paragraph views, disclosure controls, dock collapse, geometry
edits with one-step undo and returning from Gallery through the Design workspace
menu. Page thumbnail origins/clipping and the native menu mode signature were
fixed during inspection. This is Schist window QA, not external InDesign rendering
agreement; the default feature flag remains false and the item 9 fidelity gaps
below remain open.

The full 16-target sweep below passes, including workspace clippy with warnings
denied and native/browser/headless app checks: **1,670 distinct Rust tests**, all
410 editor tests, four browser tests and eight Python i18n-audit tests. The editor
target was rerun after the final label fixes. New properties cover every persisted
Design panel, legacy dock defaults, panel registration and thumbnail origins at
multiple pages, zooms and viewport offsets. Print-output proofs and Poppler checks
pass. Formatting and whitespace checks pass. Results/counts and logs are under
`/tmp/schist-design-ui-sweep-*`. The UI follow-up is on `more-indesign`.

Windows CI follow-up, 2026-10-01: run `36860614185` found one editor test
using a Unix file URL as if it were a native Windows path. The resolver correctly
rejected it. The test now uses native absolute paths, covers Windows drive/UNC
paths and localhost URLs, and verifies spaces, Unicode and literal percent escapes
decode exactly once with or without a document base. Production behavior is
unchanged. A fresh local sweep passes all 16 targets listed below, **1,666 distinct
Rust tests** (including all 408 editor tests), four browser checks and eight Python
audit tests. Formatting and whitespace checks pass. Results and counts are in
`/tmp/schist-windows-fix-results.json` and `/tmp/schist-windows-fix-counts.json`;
individual logs use the same prefix. Follow-up hosted run `36875296169` passed
Windows, macOS and Linux. PR #192 has since merged; UI follow-up work is on
`more-indesign`.

Verification checkpoint, 2026-10-01 (paragraph tabs and publication): all targets
below pass. After permissions were restored, an unfiltered `check-design` rerun
passed all 408 editor tests, including the unchanged clipboard HTTP-listener test.
The rest of the complete sweep remains valid for the unchanged source. It covers: `check-layout`,
`lint-layout`, `lint-text-directions`, `check-design`, `lint-design`,
`check-idml`, `lint-idml`, `check-separation`, `check-i18n`, `lint-all`
(`cargo clippy --all-targets -- -D warnings`), `check-layered-codecs-app`
(`cargo check -p schist-app`), `check-app-web`, `check-design-output`,
`check-editable-interchange` and `lint-editable-interchange`.
The additional headless `check-library-wasm` target also passes with
`CARGO_PROFILE_DEV_DEBUG=0`.
The browser build reused the native build's cached backer catalog because
the initial sandbox could not resolve the catalog host. A file-URL portability defect
and Design Save As filename routing were fixed before that browser check.

The initial paragraph-tab sweep recorded a PermissionDenied failure for the
clipboard listener and passed the other 407 editor tests. The publication rerun
passes all 408 without filtering, plus the layout, settings and i18n checks.
No test was modified or ignored to conceal the former environment restriction.

Distinct passing Rust tests at the publication checkpoint: editor 408, layout 349,
text engine 98, IDML 229, separation 183, core 120, settings 24 and i18n 28
(including its doctest): **1,439**. The expanded editable-interchange checks add
Affinity 39, PSD 150 and Type tools 38, for **1,666** distinct passing Rust tests across
11 crates, plus four browser i18n tests. No editor tests remain blocked. Poppler
checks verify ink patches, rotated text, sheared frames, inner image rotation
and curved/compound frame clipping, enlarged paragraph initials, vertical Japanese/Latin text, n-up reading
order, sheet counts and empty slots. The updated affine/compound-frame proof was also visually inspected.
Logs and exit codes are under `/tmp/schist-tabs-sweep-*`, with the result
index in `/tmp/schist-tabs-sweep-results.json`. The unfiltered publication rerun
is `/tmp/schist-publish-check-design.log`, with the updated result index and counts
in `/tmp/schist-publish-results.json` and `/tmp/schist-publish-counts.json`.
The list proof also passes,
and all 48 pages were visually inspected at its checkpoint. The new 24-page tab
proof passes exact plate/PDF comparisons and visual inspection; eight Python
i18n-audit tests pass. The asymmetric-offset proof also verifies PDF MediaBox/TrimBox/BleedBox with Poppler and was visually
inspected. Future changes require a new sweep.

At the paragraph-tab checkpoint, the debug app built and the native app check
passed, but Computer Use was unavailable. The UI follow-up above now records
development-window inspection. Frame affine implementation and its
verification are included in this checkpoint. Composition review found paragraph splitting/spacing/keep-rule defects. They
are fixed and covered by six new property tests and this full sweep. Inner
image transforms, native geometry, CMYK sampling and localized IDML diagnostics
are included in this checkpoint. Indented wrapping, one first-line
indent per threaded paragraph and automatic bidi context are included in this
full checkpoint. Blank-paragraph canvas carets, vertical movement, one-step
typing undo and terminal blank-line flow are included. Baseline phase alignment
and horizontal drop caps are included in this full checkpoint. Tests cover
mixed fonts, per-page and named grids, blank lines, balanced columns, grapheme
hit testing, one-step typing undo and native drop-cap counts.
`/tmp/schist-text-proof.pdf` passes independent Poppler checks and was visually
inspected.

Explicit native paragraph directions, story column order, story orientation and
automatic-direction export lowering are included in this checkpoint. Story
preferences survive edits and undo; RTL stories reverse column progression
independently of paragraph bidi. Vertical composition uses frame height as the
inline measure, supports both column progressions, and shares alignment with
preview, caret hits and print. Mixed orientations reserve separate regions;
keep chains cross their boundaries. Pixel checks use real Japanese glyphs from
a bundled font registered in memory. Vertical initials and paragraph-specific
writing-mode interchange remain unsupported. Native implicit paragraph direction
has since been corrected to LTR, preserving Schist Auto defaults through labels. Phase 0 now has v19.5/v20.2 pairs; v18/v21 and controlled changes remain missing.

The preceding sweep hit disk exhaustion. Unused temporary files, old compiler
caches and superseded editor test executables were removed. This sweep finished
without disk failures. The named-font checkpoint previously passed that HTTP test in an unrestricted
run; the publication rerun now passes it again. Source files were not removed;
current proofs and verification logs remain. The historical checkpoints below
record earlier test/publication restrictions, which have since been lifted.

The implementation spans new layout, IDML and separation crates, editor modules,
public fixtures and all 150 `design.lang` catalogs. Read `git status --short`
before changing anything; `git diff` alone omits new untracked files. Use
`git add -A` when committing the complete implementation. Do not clean, reset
or stash unrelated working-tree changes.

Where the code is:

| Area | Path |
| --- | --- |
| Layout model, geometry, styles, stories, composition | `crates/layout/src/` |
| Undoable authoring operations | `crates/layout/src/authoring.rs` |
| Pasteboard plan | `crates/layout/src/pasteboard.rs` |
| Design Mode: state, tools, hit testing, painting | `crates/editor/src/design/` |
| Design Mode: pointer and keyboard | `crates/editor/src/workspace/design_input.rs` |
| Design panels | `crates/editor/src/panels/{pages,stories,links,swatches,styles,preflight}.rs` |
| Design dock and contextual controls | `crates/editor/src/panels/{design_dock,design_controls}.rs` |
| IDML reader, writer, container, fixtures | `crates/codec-idml/`, `fixtures/idml/` |
| Design Mode documentation | `docs/design-mode.md` |
| Verified IDML facts and known gaps | `docs/idml-format.md` |

**The IDML work is clean-room.** It uses the public XML/OPC specification
and public test fixtures. The user authorized public INDD document samples
on 2026-09-29. This does not permit Adobe SDK headers or decompilation of
proprietary executables. INDD production support remains gated by Phase 0.

The received roadmap omitted its referenced numbered Phase 3 list. With
the user's authorization to complete and adjust the original plan, the
remaining order is now explicit:

1. Live rulers — implemented; guide dragging remains in item 8.
2. Preflight — implemented, with decoded graphics from item 5.
3. Separate layout Layers and page management, with reversible operations — implemented.
4. Control, Character and Paragraph controls and editable named styles — implemented.
5. Place, graphic decoding, relinking and PSD-per-page import — implemented.
6. Curve geometry in `ShapePath`, then Pen and curve editing. The model
   change gates the tool; ellipses now use the same cubic representation.
   Implemented; curve bounds, gestures and repeated native IDML saves verified.
7. Story Editor, text selection, thread controls and in/out markers — implemented.
   IME drafts commit once; text replacements preserve unaffected styles and
   refuse edits across protected structure. Native Story Editor opening,
   switching, wrapped text, paragraph breaks and one-step undo are verified.
8. Complete the design tool palette, Hand/Zoom/Eyedropper and layout guides —
   implemented. Selection drags move the whole selection
   in one step, band selection is connected, and line directions are preserved.
9. Integration and output: PDF export UI, packaging, IDML fidelity gaps,
   fixture coverage and the complete verification sweep. PDF/profile/n-up and
   package UI are implemented. Layout save-state tracking, delayed dialog guards
   and Save/Discard/Cancel transitions now cover New/Open/Close/Quit. Pages edits
   trim, margins, four-sided document bleed/slug and numbering sections. Native Color resources,
   document preferences, section numbering, style containers and tracking units
   have been corrected against public XML/specification evidence. Outer frame
   affines now preserve composition, preview/hit geometry, strokes, native CMYK
   sampling and repeated IDML saves. Independent inner image transforms now retain
   native geometry, preview/print clipping and effective resolution. Curved image
   clipping and parent-sheet geometry, overlays and scoped overrides are implemented.
   Same-spread crossovers retain paint order through preview, hit testing, IDML and
   PDF output; Select All and a single undoable drag include page contributors.
   Advanced story attributes remain explicit gaps. Package resource roles and object IDs resolve
   through XML instead of filename assumptions.
   Local supported formatting now becomes reusable named styles; native paragraph
   and structural breaks, UTF-8 offsets, paragraph font/paint defaults and opaque
   style IDs are covered. Blank paragraphs reserve height and balanced paragraphs
   no longer overlap. Browser compilation passes with a reused backer cache.
   Text wrap (TextWrapPreference, IgnoreWrap and the TextPreference wrap
   settings) composes for horizontal text and saves natively, with compact controls.
   Shaped text frames compose inside their outline, and unstyled text saves with
   the IDML root styles. Anchored page items, text frames and groups compose
   inline, above the line and at custom positions in horizontal text, and wrap
   later lines of their story. Object styles carry text wrap; jump-line page
   numbers, running headers, file names and dates render. Tables compose in
   their lines with growing rows, fills, edges and cell text.

Phase 0 research can proceed independently. Phase 5 only follows an
explicit evidence-based go/no-go; container recognition is not an INDD
layout reader or writer.


Curved graphic clipping and parent sheet/hierarchy work are included in this
checkpoint. Curved and compound outlines retain native handles, including the
public oval image fixture; preview and print preserve source alpha and CMYK.
Master spreads write actual pages, keep per-sheet geometry and overlays, and
resolve document-page and template overrides at their proper scopes. Parent text
uses its destination page's grid. Cycles and unresolved override references are
reported. Parent text threading is included in this checkpoint: master-frame links
resolve with the ordinary references, inherited frames compose their shared chain,
and page breaks advance between template sheets. Two repeated-save property tests
cover that behavior. Native GUI QA and the modern INDD paired corpus remain
unavailable; the feature stays dark.

Spread binding and LTR/RTL page-reading order are included in this checkpoint.
Moving a page selects the destination side of its facing parent without changing
the spread spine; removal preserves surviving sides. Three property tests cover
repeated native saves and one-step undo. External RTL facing and foldout specimens
remain a validation gap.

Hidden-page state and template-sheet cycle detection are included in this
checkpoint. Visibility survives IDML saves in a namespaced label with an explicit
native-visibility notice, added to all 150 locale catalogs. Acyclic parent chains
can revisit a different sheet without losing base artwork; true cycles remain
reported. Two additional property tests cover repeated saves for these cases.

Numbering sections and asymmetric bleed/slug are now implemented in item 9.
Section boundaries retain restarts, continuation, prefix visibility, name and
marker, follow their pages through edits, and save through native IDML references.
The Pages panel exposes these controls. Bleed/slug retain four edges from trim;
inside/outside follow the spread spine. Canvas geometry, separation and PDF boxes,
plate placement, media and marks share those extents. Slug offsets inside bleed
remain preserved. Per-page differences still require an IDML export notice because
native preferences are document-wide. Alternate-layout section settings remain
unsupported and are disclosed. Further native application/facing-document
validation, advanced story attributes and the modern INDD corpus remain open.

PR #192's initial CI run exposed formatting failures and a headless WASM compile
error: browser startup called the process-wide font registration API, which is
intentionally excluded from `schist_library`. The browser-only installer now has
the same configuration boundary. Workspace formatting is corrected; the library
continues using call-local font resources and its embedded fallback font.

The complete sections/offsets sweep passes. After the CI fixes, `cargo fmt --all
--check`, `make lint-all`, `make check-app-web` and the isolated
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 make check-library-wasm` also pass.
Logs for those configuration checks are `/tmp/schist-design-ci-fixes.log` and
`/tmp/schist-library-wasm-check.log`. The follow-up passed remote macOS, web and
headless-library CI; Linux/Windows workspace tests were still running when the
crossover checkpoint was prepared. New commits require their own CI run. These
checks do not establish native UI or external InDesign validation.


Cross-gutter integration is included in the current checkpoint. Page output now
includes same-spread objects reaching trim or inside bleed, with source ownership,
composition and parent-instance grids retained. Canvas paper is painted before all
artwork; drawing and hit tests share layer/object order across page boundaries.
IDML no longer reorders spread items by owning page. Select All includes current
page contributors in both views, and dragging them remains one undo step.

Eight new property tests cover source grids, scoped parent instances, selection
and undo, repeated native saves, neighbor resource preflight, and plate equality
against a whole-spread reference. The Poppler crossover proof checks continuous
text/images, transparent overlaps, adjacent artwork in inside bleed and identical
n-up placement; it was also visually inspected. All 13 targets in the current
sweep pass, including workspace clippy, native/browser app checks and PDF proofs;
formatting passes. The final editor checks were repeated after the selection fix.
No new user-facing strings or locales were introduced. Native GUI, populated
facing-master/application validation, advanced typography and the modern INDD
paired corpus remain open. The feature stays disabled by default.


Direct ink tints are implemented in item 9. Shapes retain independent fill/stroke
fractions; named paragraph and character styles inherit tint independently of
ink and opacity. Control commits shape percentages to the captured selection in
one undo step; text style fields allow blank inheritance. The eyedropper copies
tints. IDML retains direct percentages and -1 inheritance, and reports invalid
values. Named Tint swatches and object-style paint inheritance remain gaps.
Text stroke tint is retained for interchange; text stroke rendering remains
unsupported and has no new authoring control.

Separation applies tint after alias/process-build resolution, preserving one
plate per spot and full knockout coverage. Zero tint still knocks out; opacity
retains underlying ink. Property tests cover fills, strokes, text runs, aliases,
process conversion, inheritance, native saves, preview and undo. The independent
Poppler tint proof covers process/spot ramps, knockout, transparency, overprint
and styled glyphs; it was visually inspected. The three new keys are present in
all 150 locale catalogs. All 13 make targets in the new full sweep pass, including
workspace clippy, native/browser app checks and independent PDF proofs; formatting
also passes. The count is 1,208 distinct Rust tests plus four browser i18n tests.
Eleven new tests cover tint behavior; the existing eyedropper property test also
checks both tint values. Logs/results are under `/tmp/schist-tint-sweep-*`.
Temporary spec downloads and superseded tint development logs were removed; the
current proof and verification evidence remain. Native GUI/application validation
and the modern INDD paired corpus remain outstanding, and the feature stays dark.


Solid text decoration rendering is implemented in item 9. Strikethrough now
reaches preview and print; character styles retain inherited paragraph underlines
and strikes unless explicitly overridden. Character exposes a reversible
strikethrough toggle, with its label in all 150 catalogs. Decoration changes no
longer split shaping runs, preserving ligatures, kerning, wrapping and carets.
Horizontal lines use OpenType font metrics; vertical lines follow the column.

Glyphs and decorations now share coverage within each consecutive visual paint.
This fixes repeated opacity at intersections and preserves paint order across
mixed bidi text. Property tests cover inheritance, native IDML saves, text edits,
legacy serialization, spaces, both vertical directions and translucent output.
Paired PDF pages verify added decoration ink and unchanged solid glyph colours
with Poppler; horizontal and vertical proofs were visually inspected.

The shared renderer's new flag also exposed unsupported native PSD/Affinity text
encoding. Their writers now retain the existing private/pixel or reported raster
fallback for active decorations instead of emitting plain editable text. No Adobe
headers or proprietary executables were read. Custom decoration paints, weights,
offsets, line types and path decorations remain unsupported; text strokes are a
separate gap. All 15 targets in the expanded sweep pass, including the
editable-interchange tests and lint, workspace clippy and both app checks.
Formatting passes. Nine new property tests bring the Design-related count to
1,215; with the added codec/Type-tool coverage the sweep runs 1,440 distinct Rust
tests, plus four browser i18n tests. Superseded decoration development logs and
the initial proof image were removed; current proof and sweep evidence remain.
Native GUI/application validation and the modern INDD paired corpus remain
outstanding. The feature stays disabled by default.


Explicit baseline positioning is implemented in item 9. Paragraph and character
styles inherit point offsets independently; local zero resets them. Character and
Paragraph controls commit to the captured style in one undo step, and all four
new UI/diagnostic keys are in the 150 catalogs. Native IDML BaselineShift values
survive style inheritance, local formatting and repeated saves. Invalid values
and unsupported native superscript/subscript Position values are reported.

The shared renderer moves glyphs, decorations, carets, hit geometry and selections
without changing logical line spacing or uniform-shift wrapping. Canvas zoom and
output DPI scale the offsets; enlarged initials retain absolute point values and
unchanged body reservations. Selection hits use visible caret segments, including
rotated/sheared frames. PSD/Affinity use their existing fallback for active shifts
until native encoding is supported. Their zero-shift native eligibility is retained.
Automatic superscript/subscript sizing and TypePreference interchange remain gaps.

Page contribution bounds now include supported story offsets before frame
transforms. A regression test reproduced missing cross-gutter ink when the text
frame itself did not cross the gutter; both directions now match an explicitly
placed reference. The bounds are conservative and cached per story during each
page-contribution pass, without additional shaping.

The eight-page baseline proof checks horizontal, both vertical progressions and
rotated frames with Japanese/Latin glyphs, translucent colour and decorations.
Poppler independently extracts and compares every image sample after translation;
page rasterization separately verifies placement. Horizontal, vertical and rotated
pages were visually inspected. Fifteen new property tests bring this checkpoint
to 1,455 distinct Rust tests plus four browser tests. All 15 sweep targets and
formatting pass, including all 384 editor tests, workspace clippy, native/browser
app checks, editable interchange and the independent PDF proofs.

The preceding commit's Linux CI exposed a test error: the tint test treated
253/255 edge coverage as solid ink. It now checks ink divided by actual knockout
coverage, covering antialiased edges as well as solid pixels. The local separation
suite passes with that correction; remote CI must complete for this commit.
The preceding macOS, web and headless CI checks passed. Temporary spec downloads,
one-use helpers and superseded development logs were removed; current proof and
sweep evidence remain. Native GUI/application validation and the modern INDD
paired corpus remain outstanding. The feature stays disabled by default.


Named tint swatches and base-color editing are implemented in item 9. Swatches
edits native CMYK or RGB components and named percentages. One reversible edit
updates matching palette entries, ordinary/parent shapes and text styles. A named
Tint retains its full base Color definition and one spot plate; its percentage is
applied once. Direct percentage overrides detach inherited named paints to their
base, including paragraph/character inheritance. Native IDML writes Tint resources
and -1 paint inheritance. Opaque and forward references, invalid values, repeated
saves, preview, aliases, process conversion and exact undo are covered. Generated
tint names track their percentages; imported custom names remain intact.

Public-domain Penn State v20.2 templates add real seven/eight-page facing documents,
populated two-sheet parents and a spot ink. Their supported geometry, parent artwork
and text survive four saves. Four OAC v19.5 pairs add Japanese prose; only provenance
and probe metadata are committed because redistribution terms are unverified. The
corpus now has seven acquired pairs, three redistributed in the repository. Every
new INDD probe still finds XMP after an undecoded database; v18/v21, controlled
changes and database semantics remain missing. Phase 5 is still gated. The reference
PDFs were visually inspected; native application rendering remains unverified.

The new templates also expose superscript, footnote, table, math and anchored-content
gaps. Automatic superscript/subscript is implemented in the following item 9 checkpoint. Other open
work includes object-style paints, text strokes, arbitrary font variants/features,
custom decorations, alternate-layout sections and native GUI/application validation.
The feature remains disabled by default. Two swatch labels are in all 150 catalogs.
The Poppler tint proof now mixes named and direct paints and was visually inspected.

All 15 make targets, formatting and whitespace checks pass for the named-swatch
checkpoint: 1,465 distinct Rust tests across 11 crates plus four browser tests.
This includes 385 editor, 303 layout, 174 IDML and 155 separation tests. Ten new
property/fixture tests cover the changes. Logs and exit codes are retained under
`/tmp/schist-swatches-sweep-*`. Superseded development logs and the OAC archive
were removed after retaining extracted research documents and provenance.
The preceding baseline commit has passed remote Linux, macOS, web and headless
CI; Windows was still running when this checkpoint was prepared. A new commit
requires its own CI run.


Automatic superscript/subscript and native TextPreference interchange are now
implemented in item 9. The model separates Position from explicit baseline offset;
paragraph/character inheritance and local Normal resets retain both meanings.
Document preferences specify glyph size as a percentage of nominal font size and
movement as a percentage of regular leading. Nominal metrics preserve line spacing
while glyphs and insertion/selection segments scale. Canvas/output scaling and
cross-gutter contribution bounds include the derived metrics and shifts.

The shared font resolver now partitions overlapping style ranges before choosing
faces, fixing fallback runs that overwrote local glyph sizes. Editing now normalizes
those ranges with the same first-match precedence, including explicit plain gaps.
A property test compares overlapping and disjoint representations in all writing
modes and checks that a formatting edit retains each original glyph size. Native
PSD/Affinity eligibility rejects nominal-metric overrides through their existing
private/pixel or reported raster fallbacks. The native script controls and invalid
preference diagnostic are present in all 150 catalogs.

The public Penn State templates corroborate named/local superscript and document
preferences; repeated saves retain them without style growth. A nine-page proof
checks actual glyph size, regular leading and opposite script displacement through
Poppler, using Latin and Japanese in three writing modes. All pages were visually
inspected. OpenType position variants, custom decorations, text strokes, object
styles, alternate layouts and structured story composition remain open. Native UI
and external application validation remain unverified; Design Mode stays dark.

All 15 make targets, formatting and whitespace checks pass for the script-position
checkpoint: 1,475 distinct Rust tests plus four browser i18n tests, including all
386 editor tests. Ten new properties cover metrics, styling, native positions,
preferences, repeated real-template saves and one-step undo. The strengthened
shaping test enables real ligatures; normalization also covers plain ranges that
mask a later fallback. Superseded development logs were removed; the current
proofs and full-sweep logs remain under `/tmp/schist-script-sweep-*`.


Per-range OpenType shaping is implemented in item 9. Paragraph and character
styles inherit per tag, including explicit disables; the captured style field
commits one undoable edit. The shared engine preserves features through text edits
and activates shaping for local overrides, with identical feature boundaries
retaining ligatures. Native IDML boolean switches, figure styles, complete set
masks and local formatting survive repeated saves. Partial atomic groups and
arbitrary tags use reported extension labels; native edits take precedence.
Mode-dependent native CJK activation remains an explicit gap. Five new labels and
diagnostics are present in all 150 catalogs. PSD/Affinity keep the existing
fallback for unsupported native per-run settings.

The OpenType checkpoint passes all 15 make targets, formatting and whitespace
checks: 1,483 distinct Rust tests plus four browser i18n tests, including 387
editor tests. Eight new properties cover feature boundaries, real ligatures,
inheritance, syntax and undo, native masks, diagnostics and repeated specimen
saves. Existing local-formatting and raster-codec fallback properties were also
extended. The nine-page OpenType proof passes both Poppler sample and rendered
page comparisons; all pages were visually inspected. Superseded development logs
were removed; proofs and sweep evidence remain under `/tmp/schist-features-sweep-*`.
Automatic leading versus inherited point leading remained a gap at that checkpoint;
it is addressed below.
Object-style paints, text strokes, font variants, custom decorations, structured
stories, alternate layouts and native GUI/application validation remain open.


Automatic, fixed and inherited leading are implemented in item 9. Native Auto
resets inherited fixed values; paragraph percentages inherit independently.
Composition resolves Auto from each nominal run size and keeps fixed points
absolute. The shared engine separates nominal cell height from incoming baseline
or vertical center spacing, including explicit zero and empty paragraphs. First
lines fit by their font metrics; tight mixed-size paragraphs do not invent gaps.
Grids, threading, balancing, carets, preview zoom and print use those semantics.
Conservative contribution bounds retain tight-leading glyphs across the gutter.

Paragraph and Character controls accept points, Auto or blank inheritance; the
paragraph percentage field accepts 0–500. Each edit undoes once and all four keys
are present in 150 catalogs. Native IDML properties, local overrides and repeated
public-template saves preserve the settings. Legacy numeric JSON remains readable.
PSD/Affinity retain their existing fallback for unsupported absolute layer leading.

Ten new property tests and strengthened existing properties bring this checkpoint
to 1,493 distinct Rust tests plus four browser tests. All 15 targets, formatting
and whitespace checks pass, including 388 editor tests, workspace clippy and
native/browser app checks. The 12-page leading proof exactly matches independently
placed baseline/column-center references through both Poppler sample extraction
and page rendering; every page was visually inspected. Current proof and sweep
evidence remain under `/tmp/schist-leading-*`; superseded development logs were
removed. The preceding OpenType commit passed remote Linux, macOS, web and
headless checks; Windows was still running when this checkpoint was prepared.

Named font variants were the next integration gap at that checkpoint; they are
implemented below. Object-style paints, text
strokes, custom decorations, structured story composition, alternate layouts and
native GUI/application validation remain open. Public research found a v21 paired
template lead behind BOOTH sign-in, but no new acquired/version-verified sample;
Phase 5 remains gated and Design Mode remains disabled by default.


Exact named font variants are implemented in item 9. Paragraph and Character
styles inherit typographic subfamilies independently from font family and size.
A nearer legacy bold/italic choice resets an inherited named face. Resolution
uses OpenType typographic names, with legacy subfamily fallback and Unicode or
Mac Roman decoding. Equivalent spelling/casing boundaries retain ligatures.
Variable-font named instances and axis coordinates remain unsupported.

Captured style fields commit one undoable edit; unchanged legacy values remain
independently inherited. All Design property fields now accept text, fixing
previously numeric-only input for font families, Auto leading and feature tags.
Native IDML preserves FontStyle names and resource face inventories; extension
labels retain legacy independent flags, with external native edits taking
precedence. Packages list used faces, and preflight reports unavailable used
variants without flagging unused styles. Two labels are in all 150 catalogs.
PSD/Affinity retain their existing fallback for unsupported named requests.

Eight new properties bring this checkpoint to 1,501 distinct Rust tests plus
four browser tests. All 15 sweep targets, the additional headless WASM check,
formatting and whitespace checks pass, including 390 editor tests. A licensed
IBM Plex Sans Light fixture checks actual face selection against Regular. The
nine-page proof compares named and mixed faces in all three writing modes
through Poppler image extraction and page rendering; every page was visually
inspected. Current proof and sweep evidence remain under `/tmp/schist-variants-*`
and `/tmp/schist-font-style-*`; superseded development files were removed.

Four more public OAC pairs, released in September 2026, still identify InDesign
19.5 in both formats. The corpus is now eleven acquired pairs, three redistributed.
Only provenance and bounded probe metadata for the new samples are committed;
redistribution terms are unverified. Differences from the earlier templates are
not controlled single-property changes. Version 18/21 and database semantics
remain unresolved. No proprietary executable or Adobe SDK header was read.

Object-style paint inheritance is the next integration gap. Text strokes,
custom decorations, structured stories, alternate layouts and native GUI or
external application validation remain open. Phase 5 stays spike-gated and
Design Mode remains disabled by default. The preceding leading commit passed
remote Linux, macOS, web and headless checks; Windows was still running when
this checkpoint was prepared. The next commit requires its own CI.


Object-style paint work follows the named-font checkpoint and has completed
local verification subject to the clipboard sandbox restriction below. Styles now edits named object paints and inheritance, with explicit
no-ink values and enabled/disabled fill/stroke categories. Control applies local
paint to text, image and shape frames. Style apply/clear, detach, rename and
multi-object property commits are one undo step; the documented Swatches fill
exception remains. Parent artwork, swatch edits, eyedropper, paint bounds and
cross-gutter contributions share resolved paints. Frame fill precedes content and
stroke follows it. Missing graphics still report errors while available frame
paint renders. Synthetic frame paint exposes no shape-editing handles.

Native IDML references, opaque IDs, category flags, local overrides and normalized
text-frame outlines are retained. Unsupported enabled effects/categories and
stroke/corner options are reported. Curved text frame paint is supported, but text
still composes in a rectangle, with an explicit import/export notice. Full
object-style text-wrap, paragraph, effects, fitting and corner semantics remain
outside this paint subset. Text stroke rendering is implemented in the following checkpoint.

The environment changed to a restricted sandbox during this work. The unfiltered
editor run now fails its clipboard HTTP-listener test with PermissionDenied; the
same test passed at the preceding unrestricted checkpoint. The test has not been
softened. Filtered verification retains an explicit record of that omission.
The complete object-paint sweep is in `/tmp/schist-object-paint-sweep-*`: all
non-editor targets and headless WASM pass, alongside the filtered editor target.
Workspace clippy, native/browser app checks, editable interchange, formatting
and whitespace checks pass. Twelve new properties cover inheritance, explicit
no-ink, disabled categories, paint order, geometry, undo, diagnostics and control
targets. All eight proof pages were visually inspected and match independent
fill/content/stroke objects through both Poppler sample extraction and rendering.
Superseded development logs were removed. Native GUI QA remains unverified.


Git publication is blocked under the current sandbox: `git add -A` cannot create
`.git/index.lock` (Operation not permitted). The named-font checkpoint `8ce6a1eb`
was committed and pushed before that permissions change; object-paint and text-stroke work remain
uncommitted. Do not report it as part of remote PR #192 until publication succeeds.
Do not change clipboard tests to conceal the local-listener restriction.


Text stroke rendering is implemented in item 9. Paragraph and character styles
retain absolute point widths and centered/outside alignment, with independently
inherited fill/stroke inks, tints and overprint. Explicit no-fill and no-stroke
values clear inherited inks instead of falling back to black. Character and
Paragraph expose these choices; each captured style edit undoes once. Nine new
labels/diagnostics are present in all 150 catalogs. Native IDML named and local
properties survive repeated saves without creating new styles each time.

The shared renderer strokes actual font contours and scales widths at canvas zoom
and output DPI. Glyph advances, line spacing and carets remain unchanged. Outside
strokes exclude filled interiors. Equal fill/stroke inks use one silhouette to
avoid an antialiased seam and receive opacity once; distinct inks keep their paint
order and native spot identity. Preview and raster Type consumers composite the
separate paints. Cross-gutter bounds include stroke extents. Whole-text style
edits now retain paint overrides rather than taking the font-only fast path;
resolved no-stroke styles also explicitly clear prior outlines during editing.
PSD/Affinity keep their established fallback for unsupported native stroke/no-fill
settings, while zero-width strokes remain eligible for native text.

Eleven new properties bring this checkpoint to **1,523 distinct passing Rust tests
plus four browser tests**, with one additional editor test blocked by the sandbox.
All non-editor targets, workspace clippy, native/browser/headless app checks,
editable interchange, formatting and whitespace checks pass. Filtered editor
verification passes 392 tests; the unfiltered run preserves its unchanged
clipboard listener PermissionDenied failure. Logs/results are retained under
`/tmp/schist-text-stroke-sweep-*`. The twelve-page proof matches separately placed
fill-only and stroke-only text through both Poppler sample extraction and page
rendering. All pages were visually inspected. The first local-formatting test
used an absent color ID; it now uses the resource actually declared by its fixture.
Superseded development logs and one-use edit helpers were removed.

Nondefault stroke joins/miter limits, custom decorations, mode-dependent CJK
features, caps, structured story composition, alternate layouts and native
GUI/application validation remain open. The next typography integration gap is
custom decoration properties, addressed in the following checkpoint. Phase 5 remains spike-gated; v18/v21 specimens,
controlled changes and database semantics remain unresolved. Design Mode remains
disabled by default. Git publication is still blocked by the read-only `.git`
sandbox; these object-paint and text-stroke changes are not in remote PR #192.


Custom solid underline/strikethrough properties are implemented in item 9.
Paragraph and character styles independently inherit enabled state, line ink,
point weight/offset, tint and overprint. Explicit Auto resets an inherited point
value; Text color resets a named line paint to glyph fill. No ink suppresses a
line while retaining its settings. Separate line ink remains visible without glyph
fill. Character and Paragraph expose these properties with captured-target,
one-step undo. Eight new keys are present in all 150 catalogs.

Native IDML uses the published Underline*/StrikeThrough* scalar and Properties
encodings, Auto's -9999 sentinel and opaque swatch references. Named/local styles
survive repeated saves without style growth. Invalid dimensions/booleans and
unsupported pattern/gap properties are reported. PSD/Affinity retain their existing
private/pixel or reported raster fallback for unsupported custom settings.

The renderer keeps shaping, wrapping and carets unchanged. Explicit solid lines
use exact rectangle coverage, fixing direction-dependent quantization of thin
weights in the shared scanline rasterizer. Equal consecutive inks receive opacity
once; distinct inks preserve underline/glyph/stroke/strike order. Point dimensions
scale with preview/output resolution, and offset lines contribute across gutters.
The public user reference confirms horizontal offset signs; native application
agreement for automatic/vertical placement remains unverified.

Twelve new properties bring the checkpoint to **1,535 distinct passing Rust tests
plus four browser tests**, with one additional editor test blocked by the sandbox.
All non-editor targets and headless WASM pass; filtered editor verification passes
393 tests. The unchanged clipboard HTTP listener remains a recorded PermissionDenied
failure in the unfiltered run. Workspace clippy, native/browser builds, editable
interchange, formatting and whitespace checks pass. Logs/results are retained
under `/tmp/schist-custom-decoration-sweep-*`. Final review caught a test-file
collision: new properties now live in separate files and original decoration
coverage is retained. The affected full targets were rechecked and recounted.

The twelve-page PDF proof matches independent underline/glyph/strike objects in
both extracted samples and rendered pages, across three writing modes, fractional
weights, affine placement, spot tints, opacity and overprint. Every page was visually
inspected. One-use edit helpers and superseded development logs were removed.

Next integration work is nondefault glyph stroke joins/miter limits, followed by
mode-dependent CJK features and caps. Patterned/path decorations, structured story
composition, alternate layouts, advanced object styles, curved text flow and native
GUI/application validation remain open. Phase 5 is still spike-gated. Design Mode
remains disabled by default. Git publication is still blocked by read-only `.git`;
object paint, text stroke and decoration work are uncommitted and not in PR #192.


Glyph stroke joins and miter limits are implemented in item 9. Paragraph and
character styles independently inherit miter, round or bevel joins and a
nonnegative ratio. Zero bevels every nonstraight miter; absent values retain
legacy miter/four defaults. Native EndJoin/MiterLimit and local overrides survive
repeated saves. Invalid/unknown values remain reported. Character and Paragraph
edit both settings against captured targets, one undo step per gesture. Five
labels are present in all 150 catalogs.

The existing vector renderer receives the selected geometry. Join settings leave
text metrics/carets unchanged, remain dimensionless through DPI scaling and feed
conservative cross-gutter bounds. Four new properties plus strengthened native,
separation and local-formatting checks bring the checkpoint to **1,539 distinct
passing Rust tests plus four browser tests**. All non-editor targets, workspace
clippy, native/browser/headless checks, editable interchange, formatting and
whitespace checks pass. The editor passes 394 tests; its unchanged clipboard
listener is the sole sandbox-blocked failure. Results are retained under
`/tmp/schist-stroke-join-sweep-*`.

The twelve-page stroke PDF proof now exercises all three joins and zero/extended
miter limits. Extracted samples and rendered pages exactly match independently
placed fill/stroke objects; all pages were visually inspected. The old test that
called supported round joins invalid now checks unknown joins and invalid limits.
Superseded helpers, development logs and old stroke-review images were removed.

Next work is mode-dependent CJK OpenType defaults, then caps. Patterned/path
decorations, structured stories, alternate layouts, advanced object styles, curved
text flow and native GUI/application validation remain open. Production INDD is
still spike-gated. The feature remains dark, and read-only `.git` still blocks
publication of the work following 8ce6a1eb to PR #192.


Mode-dependent CJK kana and proportional-metric defaults are implemented in item 9.
Paragraph and character styles independently inherit absent/on/off settings.
Composition selects horizontal or vertical OpenType tags; nearer switches reset
inherited tags, while equally near explicit exceptions win. Captured UI choices
clear same-level exceptions in one undo step. Two keys are in all 150 catalogs.
Native IDML retains the switches, including local formatting lowered once. Private
tag exceptions remain reported and have a native-value guard so external changes,
additions or removals override stale metadata independently per group.

Seven new properties bring this checkpoint to **1,546 distinct passing Rust tests
plus four browser tests**. All non-editor sweep targets and headless WASM pass,
along with workspace clippy, formatting and whitespace checks. The editor passes
395 tests; its unchanged clipboard listener remains the sole sandbox-blocked test.
Logs/results are retained under `/tmp/schist-cjk-features-sweep-*`.

The nine-page proof matches explicit feature tags in both extracted samples and
rendered pages; all pages were visually inspected. Real Noto CJK proportional
spacing differs from full-em controls in all three writing modes. That font has
no `hkna`/`vkna` alternate glyphs, so kana activation has axis/precedence tests but
no real alternate-glyph or native-application comparison. Superseded development
logs and the preceding local PR-body draft were removed.

Next work is capitalization and small caps. Patterned/path decorations, structured
stories, alternate layouts, advanced object styles, curved text flow and native
GUI/application validation remain open. Production INDD stays spike-gated. The
feature stays dark. Read-only `.git` still blocks publishing work after 8ce6a1eb;
the current local draft description is `/tmp/schist-cjk-features-pr-body.md`.


Capitalization is implemented in item 9. Character and Paragraph expose inherited,
normal, all-caps, small-caps and OpenType all-small-caps choices, with a document
small-cap percentage. One captured choice sets both flags in one undo step; legacy
partial inheritance remains visible and preserved. Nine keys are in all 150 catalogs.
Native Capitalization/SmallCap values and local formatting survive repeated saves.
Partial legacy pairs use reported metadata, with external native edits winning.
Public themes, bounded-text and shapes fixtures now retain formerly dropped AllCaps.

Display casing keeps source bytes and grapheme caret positions, including Unicode
uppercase expansions. Ordinary small caps probes real glyph substitutions and
otherwise scales uppercase graphemes with their marks. OpenType all-small-caps
requests native features without synthesis. Explicit feature overrides win. Nominal
line cells and automatic decoration geometry remain unchanged; oversized synthetic
caps contribute across gutters. PSD/Affinity retain their established fallbacks.
Casing uses Unicode default mappings; language-tailored case mapping and native
application agreement remain unverified.

Twelve new properties bring this checkpoint to **1,558 distinct passing Rust tests
plus four browser tests**. All non-editor sweep targets, workspace clippy, native/
browser/headless checks, editable interchange, formatting and whitespace checks
pass. The editor passes 396 tests; its unchanged HTTP-listener test remains the
sole sandbox-blocked failure. Results are under `/tmp/schist-caps-sweep-*`. The
full sweep was repeated after correcting decoration continuity and cross-gutter
bounds. A test-only font-registration race was fixed by isolating fixture cache
names; existing geometry properties were retained.

The 24-page PDF proof compares uppercase expansion, native small/all-small caps
and synthetic small caps against independently authored glyph/feature controls
in all writing modes. Extracted samples and rendered pages match exactly, and all
pages were visually inspected. Superseded development logs and local PR-body
drafts were removed. Current local description: `/tmp/schist-caps-pr-body.md`.

Next work is patterned decorations and gap inks, then the remaining typography
and structured-story gaps. Path decorations, alternate layouts, advanced object
styles, curved text flow and native GUI/application validation remain open.
Production INDD is still spike-gated. Design Mode stays disabled by default.
Read-only `.git` still prevents publishing work after 8ce6a1eb to PR #192.


Striped underline/strike resources and independent gap paints are implemented in
item 9. Native Graphics.xml definitions, opaque references, unused resources,
inheritance and local overrides survive repeated saves without style/resource
growth. Different definitions may share a display name. Invalid arrays and
unsupported patterns remain diagnosed; explicit Solid resets inheritance.
Character and Paragraph expose pattern, edge percentages, gap ink/tint/overprint.
Seven keys are present in all 150 catalogs. Each captured edit undoes once;
unchanged fields preserve imported names and Solid overrides.

Stripe and gap areas are computed before each ink is quantized independently.
A regression proof caught biased half-pixel gap rounding, now fixed. Equal inks
use a single silhouette before opacity. An independent solid-band comparison also
exposed transparent-padding-dependent affine sampling, fixed by evaluating the
scale ratio first. Geometry, source text, carets and shaping remain unchanged.
Gap inks keep spot/process identity and remain visible without main-line paint.
Swatch edits include gap inks in one undo step. PSD/Affinity retain their existing
fallbacks for unsupported native pattern/gap settings.

Eleven new properties bring this checkpoint to **1,569 distinct passing Rust tests
plus four browser tests**. All non-editor sweep targets, workspace clippy, native/
browser/headless checks, editable interchange, formatting and whitespace checks
pass. The editor passes 398 tests; its unchanged clipboard HTTP-listener test
remains the sole sandbox-blocked failure. Results are under
`/tmp/schist-stripes-sweep-*`; editor checks were repeated after the unchanged-field
fix. The twelve-page stripe proof matches independently authored solid bands in
both extracted samples and rendered pages, and all pages were visually inspected.
It covers all writing modes, fractional widths, affine placement, spot tints,
opacity and opposite gap/line overprint settings. Superseded development logs and
the preceding local PR-body draft were removed. Current local description:
`/tmp/schist-stripes-pr-body.md`.

Next work is dashed/dotted and path decorations, then remaining typography and
structured-story integration. Alternate layouts, advanced object styles, curved
text flow and native GUI/application validation remain open. Production INDD is
still spike-gated; v18/v21 pairs, controlled changes and database semantics remain
unresolved. Design Mode stays disabled by default. Read-only `.git` still prevents
publishing work after 8ce6a1eb to draft PR #192.


Unadjusted butt-ended dash decorations are implemented in item 9. Native
DashArray resources use alternating point lengths, including zero members with a
positive total cycle. Named inventories, opaque references, inherited settings and
local formatting survive repeated saves. Invalid arrays and unsupported caps or
fitting remain diagnosed. Character/Paragraph expose the pattern and length
fields; unchanged imported names and Solid overrides survive focus/commit without
an undo entry. Real edits undo once. Two keys are present in all 150 catalogs.

The renderer integrates periodic coverage without enumerating repetitions, so
subnormal positive periods cannot hang. Dash phase follows visual order across
characters, spaces, bidi and paint changes. Main/gap inks remain independent.
A property test exposed inactive glyph colors changing kerning and splitting
translucent decoration groups; both now ignore that inactive color. Existing
PSD/Affinity fallback behavior remains covered. The independent reference initially
translated float geometry before rasterizing, losing near-half-pixel precision;
it now follows the local-raster/integer-placement contract. Equality stays exact.

Six new properties bring this checkpoint to **1,575 distinct passing Rust tests
plus four browser tests**. All non-editor sweep targets, workspace clippy, native/
browser/headless checks, editable interchange, formatting and whitespace checks
pass. All 398 eligible editor tests pass; the unchanged clipboard HTTP-listener
test is the sole sandbox-blocked failure in the unfiltered run. Logs/results are
under `/tmp/schist-dashes-sweep-*`. The twelve-page PDF proof matches independent
solid rectangles in extracted samples and rendered pages, covering two arrays,
all writing modes, affine placement, fractional lengths, spot tints, opacity and
overprint. Every page was visually inspected. Kernel comparisons are exact at
72/144/216 dpi both with and without affine transforms. Superseded development
logs and the preceding local PR-body draft were removed. Current description:
`/tmp/schist-dashes-pr-body.md`.

Next work is dash caps/fitting and dots, then path decorations and the remaining
typography/structured-story gaps. Alternate layouts, advanced object styles,
curved text flow and native GUI/application validation remain open. Phase 5 is
still spike-gated; v18/v21 samples, controlled changes and database semantics
remain unresolved. Design Mode stays disabled by default. Read-only `.git`
continues to block publication after 8ce6a1eb to draft PR #192.


Round/projecting dash caps are implemented in item 9, alongside the existing butt
ends. Native resource identity includes cap shape, preserving same-named variants,
unused resources, opaque references and local formatting through repeated saves.
The earlier array-only serialized representation still loads as butt-ended dashes.
Character/Paragraph cap choices and length edits preserve imported names; changed
caps undo once, while unchanged fields add no step. Four keys are in all 150 catalogs.

Rendering identifies a whole continuous segment before applying caps, so character
and paint boundaries do not invent endpoints. Projecting caps use exact intervals;
round caps use bounded adaptive circle-section integration. Overlapping caps form
one silhouette before opacity, including zero-length and extremely short dashes.
Independent properties check interval unions, circle/capsule and overlapping-circle
areas, unchanged metrics/carets and every inactive-color split. Auto cap weights now
reserve their font-derived extent in cross-gutter bounds. A separate property
requires actual neighboring-page ink for both spread sides and three resolutions.

Seven new properties bring this checkpoint to **1,582 distinct passing Rust tests
plus four browser tests**. All non-editor sweep targets, workspace clippy, native/
browser/headless checks, editable interchange, formatting and whitespace checks
pass. The editor passes 399 tests; its unchanged clipboard HTTP-listener test is
the sole sandbox-blocked failure. Results are under `/tmp/schist-dash-caps-sweep-*`.
The sweep was rerun after the bounds fix; malformed PSD/Affinity test tuples were
corrected, and the affected checks completed. IDML/clippy were repeated after the
same-name resource property was strengthened.

The 24-page cap proof exactly matches independently enumerated capsules in both
extracted samples and rendered pages. Its reference uses circle antiderivatives,
independent of production quadrature. All pages were visually inspected; final
renders match those reviewed images byte for byte. Cases cover both cap shapes,
two arrays/weights, all writing modes, affine placement, spot/process paints,
tints, opacity and overprint. Superseded development logs and the preceding local
PR-body draft were removed. Current description: `/tmp/schist-dash-caps-pr-body.md`.

Next work is dotted decorations and automatic dash/dot fitting, then paths and
remaining typography/structured stories. Public IDML figure 54 establishes dot
center spacing; the local evidence page is recorded in `docs/idml-format.md`.
Advanced object styles, alternate layouts, curved text flow and native application
QA remain open. Phase 5 remains spike-gated. Design Mode stays disabled by default.
Read-only `.git` still prevents publishing changes after 8ce6a1eb to draft PR #192.


Dotted decorations are implemented in item 9. Native DotArray center intervals
remain independent of line weight, including duplicate centers and overlap.
Names, unused resources, same-name variants, opaque references, inheritance and
local formatting survive repeated saves. Invalid arrays and automatic fitting
remain diagnosed. Character/Paragraph expose pattern and spacing controls;
unchanged fields preserve imported names, and real edits undo once. Two keys are
in all 150 catalogs. Existing PSD/Affinity fallbacks remain covered.

Four new properties bring this checkpoint to **1,586 distinct passing Rust tests
plus four browser tests**. All non-editor sweep targets, workspace clippy, native/
browser/headless checks, editable interchange, formatting and whitespace checks
pass. The editor passes 399 tests; its unchanged clipboard HTTP-listener test is
the sole sandbox-blocked failure. Results are under `/tmp/schist-dots-sweep-*`.

The twelve-page dot proof exactly matches independent analytic circle coverage in
extracted samples and rendered pages. All pages were visually inspected. Plain
and affine plates agree at 72/144/216 dpi across two weights/arrays, all writing
modes, spot/process paints, tints, opacity and overprint. The existing cross-gutter
property now also requires real dot ink on either neighboring page, for automatic
and explicit weights. Superseded development logs and the preceding local PR-body
draft were removed. Current description: `/tmp/schist-dots-pr-body.md`.

Next work is straight-segment dash/dot fitting, using the published rules for
which lengths may change. The public reference does not specify numerical
repetition selection; external application agreement must remain unverified.
Path decorations, typography/structured stories, advanced object styles, alternate
layouts, curved text flow and native application QA remain open. INDD production
remains spike-gated. The feature flag is still false, and read-only `.git` prevents
publishing changes after 8ce6a1eb to draft PR #192.


Straight-decoration fitting is implemented in item 9. Named dash resources retain
None/Dashes/Gaps/DashesAndGaps; dots retain None/Gaps/DashesAndGaps. Resource IDs
include fitting, preserving same-name variants, opaque references and inheritance
through repeated saves. Old resources default to None. Character/Paragraph edit
the captured definition in one undo step; length edits retain its name and fitting.
Five keys are in all 150 catalogs. Dash-only adjustment on a dotted resource is
still diagnosed because its public semantics are not established.

The renderer changes only the selected lengths, choosing a nearby complete dash
sequence with bounded work. Short fixed-dash spans close gaps and clip the last
dash; dash-only fitting may grow an initially zero first dash when proportional
scaling cannot fit. An independent output comparison found a remainder-rounding
bug that erased terminal dots. Fitted endpoints are now explicit; unequal-interval
properties require terminal coverage across many lengths. Whole segments are
resolved before character-owned pieces, so formatting boundaries do not refit.

Eight new properties bring this checkpoint to **1,594 distinct passing Rust tests
plus four browser tests**. All non-editor sweep targets, workspace clippy, native/
browser/headless checks, editable interchange, formatting and whitespace checks
pass. The editor passes 400 tests; its unchanged clipboard HTTP-listener test is
the sole sandbox-blocked failure. Results are under `/tmp/schist-fitting-sweep-*`.
The 24-page proof matches an independently enumerated placement search with
analytic rectangle/capsule/circle coverage. Plain and affine plates agree exactly
at 72/144/216 dpi. All pages were visually inspected, and final renders match the
reviewed images exactly. Superseded development logs and the preceding local
PR-body draft were removed. Current description: `/tmp/schist-fitting-pr-body.md`.

The public specification states which lengths may change but not how to choose
repetition counts. Schist's numerical fitting policy is documented; agreement
with InDesign remains unverified. Path-corner fitting is outside this subset.

Next is language inheritance/shaping: the existing paragraph language is retained
in styles but dropped by `ResolvedParagraph::character`, and no language reaches
the shaper. Public designmap Language resources are also not retained. Research
found both opaque Self references and native names; the 17-file corpus inventory
is at `/tmp/schist-idml-language-resources.json`. This work precedes the larger
path/structured-story connection. Hyphenation, vertical initials, alternate layouts,
advanced object styles, curved text flow and native application QA remain open.
Production INDD remains spike-gated. Design Mode stays disabled by default, and
read-only `.git` still blocks publication after 8ce6a1eb to draft PR #192.


Text languages are implemented in item 9. Paragraph defaults, inherited character
styles and local ranges reach shaping in preview and print, including ordinary
Latin text with automatic direction. Authored tags have a distinct model value;
legacy strings retain native-identity-first resolution. This fixes the case where
an imported opaque ID such as `tr` intercepted a newly typed Turkish tag. Explicit
`und`/empty resets preserve default behavior without artificial shaping boundaries.
Character/Paragraph edits retain unchanged imported dictionary identities and undo
once against the captured style. Five keys are in all 150 catalogs.

Native designmap Language resources retain IDs, names, dictionary settings, quote
pairs, numeric identifiers and labels. Known authored tags lower to observed native
names; guarded metadata retains exact tags while native references/declarations
agree. Unsupported native mappings use No Language with an explicit notice. Native
edits supersede stale metadata. Romanian locl glyphs and Turkic/Lithuanian uppercase
rules now have independent Unicode references; original text and grapheme carets
remain intact. RFC 5646 syntax checks reject malformed tags without claiming registry
validation. PSD/Affinity retain their existing private-data/pixel fallbacks.

Thirteen new properties bring this checkpoint to **1,607 distinct passing Rust tests
plus four browser tests**. All non-editor sweep targets, workspace clippy, native/
browser/headless checks, editable interchange, formatting and whitespace checks
pass. The editor passes 401 tests; its unchanged clipboard HTTP-listener test is
the sole sandbox-blocked failure. Results are under `/tmp/schist-language-sweep-*`.
The full sweep was repeated after the native-ID collision fix. All 24 proof pages
match independent Unicode text exactly; plain and affine plates agree at 72/144/216
dpi. Every page was visually inspected, and the final renders match reviewed pixels.
Eleven superseded development logs and the preceding local PR-body draft were
removed. Removing superseded test executables reclaimed 5.15 GiB while preserving
current binaries, proofs and verification logs. Current description:
`/tmp/schist-language-pr-body.md`.

Next is the text-on-path/decoration connection. The existing public
`fixtures/idml/text.idml` already has two straight native TextPath children on
Polygon parents, with independent Self/ParentStory IDs, center/baseline alignment,
RainbowPathEffect and finite start/end brackets. Exact attributes, geometry and
source hash are recorded at `/tmp/schist-text-path-evidence.json`; public DOM
references describe path brackets and alignment/effects. No path implementation
has been added at this checkpoint. Further typography/structured stories,
hyphenation, vertical initials, alternate layouts, advanced object styles, curved
text flow and native application QA remain open. Production INDD remains
spike-gated. Design Mode stays disabled by default, and read-only `.git` still
blocks publication after 8ce6a1eb to draft PR #192.


Text-on-path integration is implemented in item 9 and has completed the full
verification sweep, subject to the unchanged clipboard sandbox restriction. A Design path is a bounded, single-contour story container,
separate from raster Type tools. Control converts a shape without changing its
identity, geometry, affine or frame paint; conversion creates its story in the
same undo step. Captured start/end edits validate the whole selection, preserve
unchanged precision and undo once. Blank end follows the curve; explicit brackets
remain point distances. Duplicate now retains frame appearance and copies the
story independently. Six keys are in all 150 catalogs.

One baseline accepts one shaped line and threads into paths or boxes. Indents,
alignment, source bytes and grapheme carets stay consistent through zoom and
affines. Consecutive zero-width frame/column/page breaks preserve their destinations
and the next character; terminal blank paragraphs retain one baseline. A selection
property found inverse-affine rounding excluding the first caret, fixed with
numerical hit padding. Straight paths now use actual line segments rather than
degenerate cubics, removing noisy endpoint tangents during extrapolation.

Native Polygon/TextPath structures retain independent child IDs, stories,
thread references, brackets and cubic handles. Guarded follow-end metadata yields
to external bracket edits. Additional box/image path containers preserve primary
content and report the unsupported secondary container. Unsupported effects,
alignment/flip/spacing and invalid geometry remain diagnosed. Public native paths
survive repeated saves and actually rasterize.

Underlines and strikes now follow the curve, with solid/striped/dashed/dotted
patterns and independent gap paints. Whole-segment phase/fitting precedes bending;
equal consecutive paint fragments merge before resampling. A shared-baseline strip
mesh uses bounded miter/bevel joins and unions destination subpixel coverage before
opacity. Tests require exact cardinal coverage, subdivision invariance, bounded
reversal overlaps, independent annular geometry and unchanged text/carets across
every character boundary. Cross-gutter checks require real neighboring ink from
rotated glyphs and decorated spaces, including automatic metrics.

The 24-page path proof compares independently constructed shared-engine baseline
specifications with Design output, including alignment, opposing directions,
cubic geometry, glyph strokes, every supported decoration family, gap inks,
tints, opacity, overprint and affine placement. Kernel plates agree exactly at
72/144/216 dpi, and Poppler sample/page comparisons pass. Every updated proof page
was visually inspected. This validates shared-renderer integration, not independent
native application agreement or native corner fitting. Nineteen new properties
bring the checkpoint to **1,626 distinct passing Rust tests plus four browser
tests**. All non-editor sweep targets, headless WASM, formatting and whitespace
checks pass; the editor passes all 403 eligible tests, with its unchanged HTTP
listener test recorded as the sole sandbox-blocked failure. Logs, counts and exit
codes are under `/tmp/schist-text-path-sweep-*`. Superseded development logs and
the previous local PR-body draft were removed; current proofs, source evidence
and verification logs remain. Current local description:
`/tmp/schist-text-path-pr-body.md`.

The existing public `multipage.idml` also contains automatic numbered and bulleted
lists whose native attributes are currently dropped. Its XML and source checksum
are captured at `/tmp/schist-idml-list-evidence.json`; list composition/interchange
is the next item 9 integration gap. Other open work includes additional path
effects, hyphenation, vertical initials, structured stories, alternate layouts,
advanced object styles, curved frame text flow and native GUI/application QA.
Production INDD stays spike-gated, the feature flag remains false, and read-only
`.git` still prevents publishing changes after 8ce6a1eb.


Automatic Unicode bullets and single-level decimal lists are implemented in item 9.
Generated markers keep source bytes, grapheme carets and story editing intact;
sequence identity and explicit restarts survive wrapping and threading. Marker
character styles, alignment, tab positions, native resources and local overrides
are retained. Captured Paragraph controls undo once, preserve unchanged native
values and reject invalid input. Thirteen keys are present in all 150 catalogs.
Two long English placeholders have explicit exact-source audit deferrals, visible
in audit output and expiring when Design Mode becomes enabled by default.

Verification found empty-list caret/overset defects and marker ink outside the
frame being unclickable. These are fixed and covered across zoom, affine maps
and empty/populated paragraphs. Marker fonts participate in preflight and package
inventories; unsupported visible settings are preflight errors. The independent
24-page list proof matches manually positioned ordinary text frames exactly at
72/144/216 DPI and passes Poppler sample/page comparisons. Every page was visually
inspected. This verifies shared-renderer integration, not native application parity.

The full sweep passes except for the unchanged clipboard HTTP listener denied by
the sandbox: **1,641 distinct passing Rust tests plus four browser tests**. All
405 eligible editor tests, eight Python i18n-audit tests, workspace clippy, native/
browser/headless checks, formatting and whitespace checks pass. Results/counts
and logs are under `/tmp/schist-lists-sweep-*`; the local PR description is
`/tmp/schist-lists-pr-body.md`.

Review identified native NumberingFormat type loss and a literal caret suffix
escaping defect for the next list pass. Additional list formats/levels, cross-story
sequences, glyph-index bullets, non-left tabs and vertical/path/initial combinations
remain open. Further typography, structured stories, alternate layouts, advanced
object styles, curved frame flow and native application QA are still outstanding.
Production INDD remains spike-gated and Design Mode disabled by default. Read-only
`.git` continues to prevent publishing local changes after 8ce6a1eb to draft PR #192.


Common list formats now compose and have captured Paragraph controls: Arabic,
upper/lower Roman, upper/lower letters, three leading-zero widths and hidden
counters. Native named/enumerated formats preserve exact types and whitespace.
Literal legacy caret suffixes survive native saves. Roman range failures are
diagnosed at the actual continued number; lower-level paragraphs no longer
advance a level-one counter. Marker ink crossing either page gutter has independent
coverage tests even when the frame bounds do not cross.

The counter-format checkpoint passes **1,648 distinct Rust tests plus four browser
tests**, with all 406 eligible editor tests and eight Python i18n audit tests.
The unchanged HTTP listener test remains the sole sandbox-denied failure. All
other full-sweep targets, headless WASM, workspace clippy, native app check,
formatting and whitespace checks pass. Twenty independent list cases compare
plates at three resolutions and all 40 PDF pages pass Poppler checks and visual
inspection. Logs/results/counts are `/tmp/schist-list-formats-sweep-*`; the current
local PR description is `/tmp/schist-list-formats-pr-body.md`. Superseded list
development logs and the previous local PR draft were removed.

Item 9 continues with multilevel numbering and call-local composition caching.
Cross-story sequences, additional formats, unsupported tab settings and the
previously documented typography/story/object gaps remain open. Native GUI and
external-application agreement remain unverified; production INDD remains gated.
The feature flag is false and read-only `.git` still prevents publishing after
8ce6a1eb to draft PR #192.


Multilevel numbering is integrated in item 9. Levels 1–9 share per-story named
sequence state; higher-level references retain their own format. Parent events
restart children even when the parent number repeats, while disabled restarts
continue a level and explicit Start At takes precedence. Implicit and explicit
native default lists share one identity; equal display names do not merge IDs.
Missing/stale ancestors and unverified specific/range restart policies remain
diagnosed and preserved. Native fixture evidence currently covers only level one.

Counter results are computed once per story query, and measured marker plans are
reused across threaded columns and balance trials. Resource inventories and
preflight batch their queries. Level/restart Paragraph controls use captured
targets and one undo step; four new keys are in all 150 locale catalogs.

The multilevel checkpoint passes **1,654 distinct Rust tests plus four browser
tests**, with all 407 eligible editor tests. The unchanged HTTP-listener test is
the sole sandbox-denied failure. Every other sweep target, workspace clippy,
native/browser/headless checks, eight Python i18n audits, formatting and whitespace
checks pass. Twenty-four list cases match independent text-frame controls at
three resolutions; all 48 PDF pages pass Poppler checks and visual inspection.
This establishes shared-renderer integration, not native application agreement.
Logs/results/counts are `/tmp/schist-multilevel-sweep-*`; the current local PR
description is `/tmp/schist-multilevel-pr-body.md`. Superseded development logs
and the previous local PR draft were removed.

General paragraph tabs are the next item 9 integration gap: TabList is retained
but source tabs do not yet have tab-stop composition. Cross-story lists, further
formats, glyph-index bullets, vertical/path/initial list combinations and the
previous typography/story/object gaps remain open. Production INDD remains
spike-gated and Design Mode disabled by default. Native GUI/application QA and
publishing changes after 8ce6a1eb remain blocked by the existing environment.


### Paragraph tabs checkpoint — 2026-10-01

Ordinary source tabs now use column-relative leading stops through wrapping,
painting, carets, indents, enlarged initials, generated list markers, columns and
threading. Tabs which cannot fit remain overset and resume unchanged in a wider
frame. Justification expands ordinary spaces after the final tab without moving
earlier fields. Unmodified Tab inserts text in both the canvas and Story Editor;
selection replacements, including tabs, remain one undo step. Paragraph exposes
tab positions for ordinary text as well as lists.

Native TabList inheritance, replacements and explicit clearing survive repeated
saves. Leader and alignment-character strings now preserve literal whitespace.
Unsupported settings are retained and diagnosed. Preflight uses the original
paragraph context, avoiding false initial-tab errors on continued lines. The new
shared TextSpec field defaults to None for existing raster layers; PSD/Affinity
retain their established fallbacks when it is present.

A new proof exposed coordinate-rounding changes in translated glyph strokes.
Ordinary outlines now rasterize locally before their integer placement, preserving
exact coverage. Twelve independent fixed-position frame controls cover writing
modes, sizes, first-line indents and process/spot paints. All plates match at
72/144/216 DPI; all 24 PDF pages match extracted/reference samples and rendered
pairs and were visually inspected. Native application agreement is not claimed.

The full sweep passes **1,665 distinct Rust tests**, four browser checks and eight
Python i18n-audit tests. All 407 eligible editor tests pass; the unchanged HTTP
listener remains the sole sandbox-denied failure in the unfiltered run. All other
make targets, workspace clippy, native/browser/headless app checks, formatting and
whitespace checks pass. Logs/results/counts are `/tmp/schist-tabs-sweep-*`; the
updated local PR description is `/tmp/schist-tabs-pr-body.md`. Three new keys are
present in all 150 catalogs. The explanatory implicit-tab sentence has an exact,
visible translation deferral guarded by the disabled Design flag.

At that checkpoint, right/center/decimal tabs, leaders, native RTL/alignment/path/initial-tab
cases, cross-story and further-format lists, dictionary hyphenation, vertical initials,
structured stories, alternate layouts and advanced object behavior remained open.
Later tab and native-window progress is recorded in Handoff above; external-application
agreement remains unverified. INDD stays Phase-0-gated.
The feature remains disabled. At the paragraph-tab checkpoint, changes after
`8ce6a1eb` were local because the sandbox denied Git index writes. That restriction
was lifted for the publication checkpoint below.


### Publication verification — 2026-10-01

After the user restarted Codex with unrestricted filesystem/network access,
`CARGO_INCREMENTAL=0 make check-design` passes without filters: all 408 editor
tests, including the previously denied clipboard HTTP listener, plus its layout,
settings and i18n checks. Source code and tests are unchanged from the complete
paragraph-tab sweep. The combined verification now covers **1,666 distinct Rust
tests**, four browser checks and eight Python i18n-audit tests. All sweep targets,
workspace clippy and native/browser/headless checks pass. The old failure log is
retained alongside the passing rerun; no test was softened or ignored.

Git writes and network access are restored. The accumulated paint, typography,
path-text, list and paragraph-tab work after `8ce6a1eb` is prepared for the
`indesign` branch and draft PR #192. Historical statements above about blocked
publication describe the earlier environment. Native window/external-application
QA remains unverified, the remaining item 9 work is unchanged, and Design Mode
stays disabled by default.
