# Roadmap

Where Design Mode actually is, what gates what, and what is deliberately
not started. Written because phase status otherwise lives only in a
conversation, and the gating below is the part that is easy to get wrong:
**Phase 0 gates Phase 5**. The kernel and IDML authoring are implemented;
integration still has composition and interchange fidelity gaps to close.

## The one-line version

The layout engine and IDML open/save work. Design Mode ships dark behind
`design-mode` while the remaining Phase 3 authoring UI is completed.

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
justification, column balancing, paragraph spacing, `keep_lines`,
`keep_with_next`, indents and forced breaks. Integration review found that drop-cap geometry never reached glyph rendering
and grid leading alone did not align baselines to page guides. Horizontal
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
  open and save. The remaining Phase 3 authoring UI keeps the feature dark.
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
the remaining tools and controls below are still needed.

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
   refuse edits across protected structure. Native windows still need visual QA.
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
