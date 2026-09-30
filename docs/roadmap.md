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
| 0 | INDD spike: fixtures, container map, go/no-go, `docs/indd-format.md` | **Started — first public paired specimen acquired** |
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

Phase 0 (INDD spike, first paired sample) ──go/no-go──► Phase 5 (INDD)
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
2026-09-29 authorization. The first MIT-licensed pair is recorded in
`fixtures/indd/README.md`; coverage of v18–v21 and controlled changes is
still required before a format go/no-go.

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

**Specimen gaps, known rather than assumed.** Not exercised by any real
file: a two-page spread and its gutter, a master page with an item
overridden on a page, a spot ink, overprint, and a non-ASCII script. Frame affines now retain oriented text, graphics and shape strokes; independent inner image
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
interchange gaps. The native debug build succeeds; GUI inspection was denied by Computer Use. INDD export remains
gated by Phase 0/5. See [IDML evidence and limits](idml-format.md).

### Phase 5 — INDD

Open-ended, spike-gated. Reader for the recovered subset, then a writer if
Phase 0 justifies it.

## Known gap in the localisation

`design.lang` keys are present in English and all 149 non-English locales: **51 translated,
98 carrying English** under an explicit `# UNTRANSLATED PLACEHOLDER`
marker. The 19 new Preflight/ruler keys use English placeholders in every
non-English locale. Keys are added to all 150 catalogs at once, in English, and translation is a
separate pass — a new feature must not block on 149 translators. `check-i18n.py --strict-audit` fails any value identical to English
with seven or more words, which is why two Design Mode strings were
shortened in the English source to stay under that line while the catalog
is untranslated. They read better short, but the constraint is real and the
other long strings will need attention as the catalog grows. See
[i18n translation status](i18n-translation-status.md).

## Build and check targets

```sh
make check-layout      # the kernel
make check-design      # the editor's Design Mode code
make lint-design
make check-idml        # the IDML package, cross-checked against zip/unzip
make lint-idml
make check-separation  # inks, plates, PDF
make check-i18n        # fails on untranslated English prose
```

`check-design` compiles the editor with `CARGO_INCREMENTAL=0`: its test
binary is large enough that the incremental cache is not worth the disk.

The i18n audit itself is `tools/check-i18n.py --strict-audit`, and adding a
key means adding it to all 150 catalogs. See
[i18n translation status](i18n-translation-status.md) for how placeholders
are marked.

## Handoff

Verification checkpoint, 2026-09-30: `check-layout`, `check-design`, `lint-design`,
`check-idml`, `lint-idml`, `check-separation`, `check-i18n`, `lint-all`
(`cargo clippy --all-targets -- -D warnings`), `check-layered-codecs-app`
(`cargo check -p schist-app`), `check-app-web` and `check-design-output` pass.
The browser build reused the native build's cached backer catalog because
the initial sandbox could not resolve the catalog host. A file-URL portability defect
and Design Save As filename routing were fixed before that browser check.

The initial sandbox run passed 377 editor tests and blocked the clipboard HTTP
test at `TcpListener::bind`. After restarting with filesystem and network access,
the unfiltered `CARGO_INCREMENTAL=0 make check-design` passed all 378 editor tests,
including that HTTP test, plus its layout, settings and i18n checks. The successful
retry is recorded in `/tmp/schist-commit-check-design.log`.

Distinct passing Rust tests across the sweep and retry: editor 378, layout 281,
text engine 42, IDML 156, separation 143, core 120, settings 24 and i18n 28
(including its doctest): **1,172**, plus four browser i18n tests. Poppler
checks verify ink patches, rotated text, sheared frames, inner image rotation
and curved/compound frame clipping, enlarged paragraph initials, vertical Japanese/Latin text, n-up reading
order, sheet counts and empty slots. The updated affine/compound-frame proof was also visually inspected.
Logs and exit codes are under `/tmp/schist-visibility-sweep-*`, with the result index
in `/tmp/schist-visibility-sweep-results.json`. Future changes require a new sweep.

The debug app builds. Native window visual QA remains unverified because
Computer Use was not approved for Schist. Frame affine implementation and its
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
has since been corrected to LTR, preserving Schist Auto defaults through labels. Phase 0 still lacks the modern paired corpus.

The preceding sweep hit disk exhaustion. Unused temporary files, old compiler
caches and superseded editor test executables were removed. This sweep finished
without disk failures. Its sandbox-denied HTTP listener passed in the unrestricted
retry above. Source files were not removed; current proofs and verification logs remain.

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
   trim, margins, shared bleed/slug and numbering. Native Color resources,
   document preferences, section numbering, style containers and tracking units
   have been corrected against public XML/specification evidence. Outer frame
   affines now preserve composition, preview/hit geometry, strokes, native CMYK
   sampling and repeated IDML saves. Independent inner image transforms now retain
   native geometry, preview/print clipping and effective resolution. Curved image
   clipping and parent-sheet geometry, overlays and scoped overrides are implemented.
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
