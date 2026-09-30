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
| 0 | INDD spike: fixtures, container map, go/no-go, `docs/indd-format.md` | **Started — seven public pairs acquired; database semantics unresolved** |
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

Phase 0 (INDD spike, seven paired samples) ──go/no-go──► Phase 5 (INDD)
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
2026-09-29 authorization. Seven acquired pairs now cover an older version,
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

Verification checkpoint, 2026-09-30: all 15 targets pass: `check-layout`,
`lint-layout`, `lint-text-directions`, `check-design`, `lint-design`,
`check-idml`, `lint-idml`, `check-separation`, `check-i18n`, `lint-all`
(`cargo clippy --all-targets -- -D warnings`), `check-layered-codecs-app`
(`cargo check -p schist-app`), `check-app-web`, `check-design-output`,
`check-editable-interchange` and `lint-editable-interchange`.
The browser build reused the native build's cached backer catalog because
the initial sandbox could not resolve the catalog host. A file-URL portability defect
and Design Save As filename routing were fixed before that browser check.

The unfiltered `CARGO_INCREMENTAL=0 make check-design` now passes all 388 editor
tests, including the clipboard HTTP test that the earlier sandbox blocked, plus
its layout, settings and i18n checks.

Distinct passing Rust tests at the leading checkpoint: editor 388, layout 311,
text engine 55, IDML 184, separation 156, core 120, settings 24 and i18n 28
(including its doctest): **1,266**. The expanded editable-interchange checks add
Affinity 39, PSD 150 and Type tools 38, for **1,493** distinct Rust tests across
11 crates, plus four browser i18n tests. Poppler
checks verify ink patches, rotated text, sheared frames, inner image rotation
and curved/compound frame clipping, enlarged paragraph initials, vertical Japanese/Latin text, n-up reading
order, sheet counts and empty slots. The updated affine/compound-frame proof was also visually inspected.
Logs and exit codes are under `/tmp/schist-leading-sweep-*`, with the result
index in `/tmp/schist-leading-sweep-results.json`. The asymmetric-offset
proof also verifies PDF MediaBox/TrimBox/BleedBox with Poppler and was visually
inspected. Future changes require a new sweep.

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
has since been corrected to LTR, preserving Schist Auto defaults through labels. Phase 0 now has v19.5/v20.2 pairs; v18/v21 and controlled changes remain missing.

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

Named font variants are the next integration gap. Object-style paints, text
strokes, custom decorations, structured story composition, alternate layouts and
native GUI/application validation remain open. Public research found a v21 paired
template lead behind BOOTH sign-in, but no new acquired/version-verified sample;
Phase 5 remains gated and Design Mode remains disabled by default.
