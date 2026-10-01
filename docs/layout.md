# Design Mode: page layout

`crates/layout` is the kernel for Design Mode, Schist's page layout mode.
It describes pages, spreads, frames, text and print rules. It has no UI
types and no dependencies on `gpui`, so it is testable on its own and
compiles for the browser like the rest of the kernel.

## Two models, on purpose

`schist_core::Document` is a raster image with layers. `schist_layout`
is a document with pages and frames. They are not two views of one
thing.

A placed photograph is a **link** from a graphic frame to a raster
document. Editing the photo never rewrites the layout that references
it, and deleting a frame never touches the photo. That is the whole
reason a page layout application does not open a PSD when you place one,
and it is why a folder of PSDs imports as a set of linked pages without
copying a single pixel into the layout file.

```
LayoutDocument ──▶ GraphicFrame { link: Link { path, modified, present } }
                                        │
                                        ▼
                            schist_core::Document (raster)
```

## Units

Points, 1/72 inch, everywhere. Millimetres and inches are input and
presentation conveniences that convert at the edges; the inch is exactly
25.4 mm by definition, so `mm(210.0)` and `inch(8.5)` are exact.

Leading and all other typographic distances are points, not pixels. A
page style states leading as an absolute distance, because that is how a
designer thinks about it and how it has to survive a change of font. The
text engine takes a multiple of the font's own line gap, so the
conversion goes through the engine's real metrics rather than by
dividing by the point size.

InDesign's own formats store 8.24 fixed point. That conversion belongs
to the codec crate, not here.

## Coordinates

An object's `bounds` are in **page space**: relative to its own page's
top-left corner, not the spread's. A paragraph style with an indent has
to mean the same thing on page 1 and page 200, and reordering a spread
must move content without rewriting any coordinates. Use
`LayoutDocument::object_rect` to get spread-space or pasteboard-space
geometry for drawing and hit-testing.

## Modules

| Module | What it holds |
|---|---|
| `geometry` | `Point`, `Rect`, `Insets`, `Page`, `Spread`, `ShapePath`, page number styles, unit conversion |
| `model` | `LayoutDocument`, `LayoutObject`, `PlacedObject`, `ParentPage`, `Link` |
| `styles` | `ParagraphStyle`, `CharacterStyle`, `StyleSet`, and inheritance resolution |
| `story` | `Story`, `Point`, `StyleRange` — the linear text flow |
| `ink` | `Ink`, `InkAlias`, `InkManager`, UCR and black generation |
| `compose` | Fitting a story into columns and threading it across frames |
| `grid` | Document grids, baseline grids, snapping and guide positions |
| `history` | `LayoutEdit`, `History` — the undo stack |
| `edit` | Applying and reversing edits against a real document |

## Stories and threading

This is the idea that separates page layout from a word processor. Text
does not belong to a frame; it belongs to a **story**, and any number of
frames can show any part of it. Three frames on three pages holding one
article is one story, not three, which is why editing the first changes
the other two.

`compose_thread` walks a thread of frames in order. Each takes what
fits; the rest carry the overflow. The union of all frames' contents is
exactly the story: nothing duplicated, nothing skipped.

Two facts are reported per frame, and they are not the same thing:

- `passed_on` — this frame ran out of room and handed the remainder to
  the next frame. A three-page article overflows twice and loses
  nothing.
- `lost` — text did not fit anywhere. This is what preflight reports.

Conflating the two would make a working multi-page article look like a
failure, and would make a genuinely dropped paragraph look fine.

## Styles

Paragraph and character styles inherit through `based_on`, and a
paragraph style names its successor through `next`, which is what makes a
heading style chain into body copy.

Every style property is `Option`, meaning "inherit". That keeps a style
cheap to define and safe to edit: adding a property does not invalidate
every style in every document, because unset stays unset. Resolution
walks the chain from the most specific style outwards, so the nearest
definition of a property wins. A cycle or a dangling `based_on`
terminates rather than looping, because a file from another tool can and
does contain both.

## Inks, and why this is not `schist_core::ink`

`schist_core::ink::InkChannel` is a *painted plate*: a scalar coverage
buffer you brush ink into, already registered to the raster, already
round-tripping through PSD as a DisplayInfo alpha channel. That is the
right model for retouching a channel.

Page layout works on a different axis. An object simply says "my fill is
PANTONE 032 C", overprint is a per-object property, and the plate is
*derived* at output time by asking which inks touched each pixel. So the
two coexist:

- `ink::Ink` names a colour — spot or process, defined in Lab, because
  that is the only space in which a Pantone book, a CMYK build and a
  screen value can be compared without a profile doing the work.
- `ink::InkManager` holds the output-time decisions: separate or convert
  to process, alias two names onto one plate, under-colour removal,
  black generation, total area limit.
- `crates/separation` resolves those and emits `schist_core::InkChannel`
  plates, which inherit the existing registration, undo, CRDT and PSD
  write for free. See [print separation](separation.md).

`InkManager::resolve` follows alias chains and stops at a cycle rather
than hanging preflight. `Ink::preview_rgb` is a screen approximation and
is never used for output.

`Ink::to_cmyk` is preview-grade — see [print separation](separation.md)
for why that matters, and for why the CMYK build is injected rather than
computed here.

## Composition

`compose_thread` walks a thread of frames in order. Each takes what
fits; the rest carry the overflow.

### Justification

The text engine measures natural widths, so it cannot justify. Composition
computes the stretch and puts it on the line as
`ComposedLine::word_space`; the renderer applies it. Keeping it as data
rather than a baked coordinate is what lets a renderer decide whether to
honour it.

A line is not stretched when it is ragged, when it is the last line of
its paragraph, when it is already at or past the measure, or when it is a
single word with no space to stretch. The last case matters: a lone word
left short is the one thing justification cannot help, and stretching it
anyway looks worse than leaving it.

`JustifyAll` stretches the last line too.

### Column balancing

A frame whose whole story fits is balanced by finding the smallest column
height that places every whole paragraph. Each candidate uses the same
spacing, keep-chain and grid rules as ordinary flow. Paragraphs are not split
just for balance; an oversized paragraph falls back to ordinary flow.

A frame that *overflows* is not balanced. Text has to keep going, so the
columns fill in order and the last is short on purpose. Rebalancing that
would leave a gap in the middle of an article, which reads as a mistake.

### Paragraph features

| Feature | Behaviour |
|---|---|
| `space_before` / `space_after` | Gap along the block axis, dropped at a column's start and after a frame's last text |
| `keep_lines` | The widow rule: neither side of a break is left with fewer than N lines |
| `keep_with_next` | A heading is not left stranded at the foot of a column |
| `drop_caps_lines`, `drop_caps_characters` | Horizontal opening graphemes are shaped and painted once at the enlarged size; the first N body lines clear their measured ink |
| indents | Narrow the measure, and first-line indent applies to line one only |
| `direction` / `writing_mode` | Paragraph bidi is independent of story column order; writing mode falls back to story orientation |

A drop cap of one line reserves nothing: that is an ordinary capital, and
reserving space for it would leave a hole where the letter goes.

### Grids

`grid::GridSettings` is a **document grid** (columns, rows, margins) and
a **baseline grid** (a rhythm of horizontal lines at a fixed interval).
Text set *on* the baseline grid has every baseline on a line, so text
flowing between frames across pages keeps one rhythm — which is the
entire reason a multi-page article reads as one document rather than a
pile of pages.

Snapping retains the style's minimum advance and moves each measured baseline
forward onto a guide. LinesPerGrid first rounds the line advance up to a whole
number of intervals. The resulting advance reaches rendering and caret
measurement as well as placement; otherwise their line heights disagree with the
positions, and text drifts off the grid a line at a time.

`GridSettings::snap_y` and `snap_rect` are for dragging. A zero or
negative `baseline_count` produces no grid at all, because dividing by it
would put an infinity into every line position.

## Undo

`History` stores **operations, not snapshots**. A layout document is
large enough that a hundred pages of text is a few megabytes, and a
slider drag would make one of those per mouse event. Each edit is small
and reversible, and applying the inverse restores the previous state.

`edit.rs` writes each operation's forward and inverse as one function
each, so the two cannot drift apart. An undo that is not the exact
inverse of its edit is how a document ends up subtly wrong after a few
undo/redo cycles and nobody can say why.

Page edits carry the **whole** spread list, before the change. Removing a
page renumbers every spread after it, so recording one spread is not
enough to make the inverse exact.

The undo tests are round trips: apply, undo, require the document
byte-identical to what it was. An inverse that is nearly right is worse
than no undo, because it is very hard to notice.

## Verification

`make check-layout` runs the tests, `make lint-layout` runs clippy, and
`make check-layout-wasm` type-checks for the browser build.

The tests that matter most are the ones pinning invariants that are easy
to break silently:

- lines never exceed their frame's height, and a column's line budget
  holds across every pass `fill_column` makes;
- a story split across a thread has no gaps and no repeats, and each
  frame resumes exactly where the previous stopped;
- each paragraph keeps its own style through composition, and character
  ranges reach the lines they cover;
- a narrower column needs more lines, and text wraps to the column width;
- `Story::text_len`, `point_offsets` and `slice` agree on one byte
  coordinate system;
- style inheritance terminates on a cycle, and a dangling `based_on` is
  ignored;
- the ink alias resolver terminates on a cycle, and UCR never raises
  total ink coverage;
- a justified line's spaces add up to exactly the slack, the last line
  of a paragraph is left ragged, and a paragraph split across columns
  keeps its last line justified;
- columns are balanced when the story fits and left in order when it
  overflows;
- a baseline grid sets the leading to its interval, and a grid off
  leaves the style's alone;
- a split paragraph keeps `keep_lines` lines on each side, and a heading
  is never stranded;
- a corrupted `baseline_count` produces no grid rather than an infinity;
- every undo round trip returns the document byte-identical, including
  page edits that renumber spreads, objects and parent pages.

## The pasteboard plan

`schist_layout::pasteboard` is the seam between the layout kernel and
anything that draws. It takes a document and a `PasteboardView` and
returns a `Pasteboard` of paper boxes, guides, frames, composed text runs,
shapes, graphics and notes — in pasteboard points, already scaled for the
canvas.

It is display data and nothing more: it cannot be edited, and each frame
carries its `ObjectId` so a caller can answer "what did I click" exactly
rather than inferring it from geometry. The editor's `design::paint` and
`design::select` consume it, and neither computes layout.

## Spread placement

`Spread::pages` lists physical slots from left to right; `LayoutDocument::pages`
keeps reading order. `PageBinding` controls native reading progression independently
of text direction. An explicit `Spread::binding_location` counts slots left of the
spine; otherwise facing-page defaults determine it. Page reordering retains the
slots and reselects facing parent sheets by destination side. Removing a slot
updates the spine and every page reference in the same undoable topology edit.

`LayoutDocument::spread_origins` lays the spreads out left to right,
separated by `SPREAD_GAP`, and the pasteboard uses it in preference to the
stored `Spread::origin`. That field is zero for every document built
through the API, so trusting it drew each spread exactly over the first —
a document looked like half its length. It is kept for a document that
stores its own layout, and is documented as not consulted when drawing.

See [Design Mode](design-mode.md) for the editor side.

## Affine frame placement

`PlacedObject.bounds` is the untransformed composition box. `transform` applies
about that box's origin, followed by `rotation` about its center. Moving bounds
translates artwork one-for-one; resizing reflows text in local coordinates.
`content_transform` maps composed page coordinates to visible page coordinates;
`visual_bounds` and inverse-map hits agree with that map. Snapshots and duplicate
operations preserve both fields, and old serialized objects default to identity.

Preview and print share bounded inverse mapping and premultiplied interpolation.
Interpolation returns straight channels, so a transparent edge cannot dilute
native CMYK values. Stroke outlines transform with their shapes. Curve edits
rebase local geometry while preserving the page-space positions of untouched
anchors and handles. Preflight rejects singular/nonfinite transforms and derives
effective image resolution from the largest stretch of the placed pixel grid.

Graphic frames also carry `image_transform`, expressed in normalized frame
coordinates. It applies after `image_rect` fitting and before frame clipping;
then the outer object affine moves the clipped result. Resizing preserves its
normalized placement. `ImageMapping` supplies the same inverse map for preview
and separation sources. The crop rectangle controls fitting, not a second mask.
Legacy serialized frames default to identity. Relink, duplicate and undo retain
both transforms. The shared affine inverse uses f64 intermediates to avoid
overflow/underflow of a finite f32 determinant.

## Paragraph flow invariants

Frame boundaries select complete lines from the text engine's measurement of
the paragraph. Prefix bisection previously shortened lines and split words.
Each line now reserves its own measured height, including mixed font sizes.
An unfinished paragraph ends the column; smaller later paragraphs cannot skip
over its remaining text. Both sides of a split must satisfy `keep_lines`.
A chain of `keep_with_next` paragraphs is provisional until a following
paragraph actually places text, so spacing and differing font sizes cannot
strand a heading. Tests exercise these rules across capacities and column counts.

Indents affect shaping as well as line placement. The text engine accepts
successive inline measures, with the final measure repeated; this keeps full
paragraph shaping context while allowing a distinct first-line width. A block
retains its original paragraph start so a frame continuation cannot repeat the
first-line indent. Automatic direction resolves from the original paragraph
before any frame or display-line slicing, keeping mixed-direction caret and
glyph positions consistent through the thread.

Empty paragraphs retain a canvas text slot for hit testing, typing and vertical
caret movement. A terminal empty paragraph reserves a line even though it has
no source bytes; the composer uses one internal flow position for it and clamps
every emitted range and consumed offset to the real text length. An untouched
empty story has no overset text. Preview, print and caret navigation use the
shared `line_spec` helper, which turns reserved blank lines into empty specs
instead of painting a newline as an extra row.

Baseline placement uses each frame's page margins and named grid. Snap mode moves measured baselines forward onto fixed guides without compressing the requested line advance; LinesPerGrid rounds advances up to whole intervals. Blank lines and balanced columns use the same placement budget. Horizontal drop caps retain their own source ranges for caret editing, stay with the covered body lines, and render through the shared preview/print specification. Vertical drop caps are not yet composed.

Vertical stories wrap to frame height and advance across its width. Insets,
paragraph spacing, blank lines and balanced frame columns use logical inline
and block axes. A paragraph writing-mode change starts a new region in the
remaining rectangle; keep-with-next chains cross these transitions. Preview,
print and caret origins share inline alignment. Up/Down follows vertical text,
Left/Right changes its column, and hit tests retain grapheme boundaries under
zoom and frame affines. The PDF proof uses the bundled Noto Sans CJK JP font,
registered in memory rather than installed in the user's font directory.


Graphic frames can carry a normalized `ShapePath` clip. Its anchors and handles
resize with the frame, independently of image fitting and the inner image affine.
Preview and separation share its destination-space antialias mask; clipping only
multiplies coverage and never converts the source colour channels.

Parent templates retain their individual sheets and spread origins, with a
sheet choice and overlay for each document-page application. Rendering resolves
base templates first and then their children, applying scoped overrides and
composing overlays on temporary objects. Shared composition boxes remain unchanged.
Page insertion, removal and reordering remap the applications in the same undo
transaction. Parent text composes against the destination page's baseline grid.

### Page numbering and outside-trim geometry

`Page.section` carries an optional numbering boundary; `numbering::Section`
stores restart/continuation, style, prefix visibility, name and marker. The first
page has an implicit Arabic section starting at 1 when no boundary is present.
`section_at`, `page_number_value` and `page_number` use document reading order,
including hidden pages. Boundaries follow their starting page through moves and
undo; deleting that page removes the boundary, and inserting a page copy clears
its boundary. Section edits use page snapshots, with no duplicate global
numbering state in document settings.

`Page.bleed` and `Page.slug` are physical top/right/bottom/left `Insets`, measured
from trim. The bleed rectangle expands each trim edge independently; media uses
the larger bleed/slug extent per edge. A slug inside bleed is retained as authored
settings rather than reduced to zero. The Pages controls translate inside/outside
through the spread spine, while the kernel retains physical offsets on each page.
Per-page offsets can differ; IDML's global preferences require an explicit export
notice and per-edge expansion in that case, including after moving pages with
different physical offsets between spine sides.


### Artwork crossing page boundaries

Ownership and rendering have separate queries. `page_objects` lists the page's
own objects and applied parent instances. `page_artwork` also includes neighboring
instances whose painted bounds reach the requested trim/bleed box, translated
from the same spread. The translation changes only the final affine: the original
composition box, source page and baseline grid remain intact. Stroke extents are
included, and other spreads never contribute. Parent overrides remain scoped to
the source instance, so suppressing a right-page instance does not suppress a
left-page instance's crossover.

The pasteboard paints every paper box before artwork, then uses one order across
pages for drawing and hit tests. Layers retain priority; within a layer, inherited
artwork precedes ordinary objects and ordinary insertion order remains stable.
Single-page views include neighboring crossovers once. Select All includes the
current page’s contributors in either view; a selection drag changes all of them
in one undo step. Page ownership does not
change merely because an object is visible or selected across a gutter.
