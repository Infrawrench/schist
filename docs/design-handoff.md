# Design Mode handoff — 2026-10-05

Read `AGENTS.md`, then `docs/roadmap.md` in full. The roadmap is authoritative;
do not recreate the plan from the original conversation. This handoff supplements
its latest checkpoint for work on another machine.

## Publication

Branch: `design-tab-leaders`. Existing draft PR:
https://github.com/Infrawrench/schist/pull/195

The latest commit composes tables (first phase). It follows `90f6ce88`
(file-name and date variables), `c29ff40b`
(running headers and jump numbers inside threads), `b5615e0c` (jump-line page numbers, object-style wrap, layer IgnoreWrap),
`dff813d6` (wrap around anchored items), `29e24efa` (anchored text
frames and groups), `9c757f22` (above-line and custom
anchored positions), `1a8853b3` (inline anchored
items), `9c516fb9` (text in shaped frames
and IDML root styles), `7dc6b99c` (text wrap), `550ba45f` (native
page numbers, section markers, chapter-number variables and the unsaved-close
fix), `021e3bbd` (last-page-number variables) and `aaad6dc0` (custom-variable
authoring). Fetch the branch head.

## Current work

Phase 3 items 1–8 are implemented. Item 9, output and interchange integration,
remains open. Design Mode is a separate `LayoutDocument` editor; it stays disabled
by default behind the `design-mode` feature flag.

Text variables have three typed kinds: literal custom text and the native
LastPageNumberType and ChapterNumberType. Native `<?ACE 18?>` page numbers and
`<?ACE 19?>` section markers are typed controls with native output, and the
document ChapterNumberPreference is retained. A last-page value is evaluated for the pages a composition pass
can occupy: a parent instance uses its destination page; an ordinary thread renders
a section value only when every frame shares one section. Ambiguous scope, visible
section prefixes and unrendered formats (Kanji, full-width, leading zeros) stay
unrendered and diagnosed. The compact Text Variables window switches kinds with two
icons; computed drafts show Text before/after with a Format dropdown, plus Scope
for last page numbers. Two icons insert a current page number or section marker
at the captured cursor. Current page numbers and markers use the same page
context. Next/previous page numbers follow their own thread from the frame they
land in (composition reruns with `marker_frames` until stable), or in a one-frame
story the thread of the story frame their frame touches
(`text_variables::jump_page_value`). Running headers (`RunningHeader`,
`running_headers.rs`) search other stories' composed lines for the page. Date and
file-name variables read `LayoutDocument::dates` and `file_path`, which the
editor stamps at open, save and output (`design::now`); `dates.rs` formats.
Tables (`tables.rs`, `table_codec.rs`) are typed with cell stories appended to
`doc.stories`; `tables::layout` sizes rows by composing cells as detached frames
and `tables::objects` gives fills, edges and cell frames for the anchored path.

Text wrap is modeled per page item (`ObjectAppearance.text_wrap`), per text
frame (`ignore_wrap`), per layer (`LayoutLayer.ignore_wrap`) and per document
(`text_wrap_preferences`), and interchanges with IDML TextWrapPreference,
IgnoreWrap and the TextPreference wrap attributes. Horizontal text composes
around bounding boxes, contours, jump-object and next-column wraps, with sides,
spine sides, inverse outlines, offsets, rotated/skewed frames and spread
artwork crossing the gutter. Each free interval of a line band is one measure for
the per-line measure engine. Vertical text, initials, list markers and path text
compose unwrapped with a Preflight warning; pixel contours use the item outline
with a Preflight warning. The Control panel shows five mode icons, an ignore-wrap
toggle for text frames, a uniform offset, an inside-outline toggle and a side
dropdown; Text Preferences carries the two document toggles. The canvas draws a
dashed boundary for selected box-shaped wraps.

A text frame with a non-rectangular outline composes inside it: the outline is an
inverse wrap of the frame's own shape inset by its top inset, sharing the wrap
bands, with holes following the path's fill rule. "Text in shape" in the
Properties panel turns a selected closed shape into such a frame in one undo
step. Unstyled runs, controls and list markers save as
`CharacterStyle/$ID/[No character style]`; Styles.xml always defines both root
styles, and on reading the root character style (and a bare root paragraph style)
is not a document style.

Page items inside stories (Rectangle, Oval, Polygon, GraphicLine) keep their
retained XML and are also typed as `StoryStructure.anchored`: AnchoredObjectSetting
position, Y offset and the item read by the spread-item reader. The text engine has
inline boxes (`TextSpec.inline_boxes`): a U+FFFC inside an inline object with its
own width, ascent and descent that draws nothing, wraps whole and reports its
position. Composition projects each inline item in horizontal text as such a box
the size of its visual extent (stroke included), places it on the baseline raised
by its Y offset, and draws it through the frame's affine after the frame's text in
output and on the canvas. Auto leading steps the item's line by its height plus
the text's extra leading; fixed leading keeps its step. These follow InDesign's
PDF of the public paged-media `anchored` sample (see `docs/idml-format.md`),
whose other pages also measure above-line and custom positions.
Boxes must be scaled with their specs (`TextSpec::scale_inline_boxes`) wherever a
spec is scaled. Every AnchoredObjectSetting attribute is typed
(`anchored::Placement`). Items above the line are zero-width boxes whose `above`
room lowers their line; custom items are zero-width boxes placed against the
anchor, line metrics (`InlineBoxPosition` reports the anchor font's size, ascent,
cap height and x-height), column, frame, margins or page. Anchored text frames
compose their story in their own box (`compose_object` already composes a frame
outside `doc.objects` as a one-frame thread) and draw as generated text of the
holding frame; groups are flattened into `AnchoredItem::members`. Items live in
Schist's structured-story retention record, not native story XML; placed copies
take the reserved id `anchored::PLACED`, which no document object has. Items at
custom positions wrap their own story's lines after the anchor's line
(`PreparedStory::anchored_wraps`, recomposed until settled); other stories'
frames they reach get "wrap not applied to some text". Items in vertical text
stay reported as unrendered.

Key files:

- `crates/layout/src/text_wrap.rs`: model, band geometry and undoable edits.
- `crates/layout/src/text_shape.rs`: attaching text inside a closed shape.
- `crates/layout/src/compose.rs`: `place_wrapped`, `plan_slots`, blank lines.
- `crates/codec-idml/src/text_wrap_codec.rs`: native read/write.
- `crates/editor/src/panels/design_wrap.rs`: compact controls.
- `crates/layout/tests/text_wrap.rs`, `crates/codec-idml/tests/text_wrap.rs`,
  `crates/separation/tests/text_wrap.rs`: geometry, interchange and plate ink.
- `crates/text-engine/src/lib.rs` (`InlineBox`, `inline_box_positions`) and
  `shaping.rs`: engine inline boxes.
- `crates/layout/src/anchored.rs`: typed items, line boxes, placement through the
  frame; `footnote_composition.rs` projects them, `pasteboard.rs` and
  `crates/separation/src/separate.rs` draw them.
- `crates/codec-idml/src/anchored_codec.rs`: typing items from retained XML.
- `*/tests/anchored_items.rs`, `crates/layout/tests/anchored_positions.rs` (the
  native sample's pages), `crates/layout/tests/anchored_frames.rs`,
  `crates/layout/tests/anchored_wrap.rs` and
  `crates/text-engine/tests/inline_boxes.rs`.

## Continue in roadmap order

Next: tables breaking across frames with repeated header/footer rows and
KeepWithNextRow (`tables-rows` PDF pages 8–10), then table and cell styles and
alternating fills (`tables` PDF pages 3–6), then the remaining item 9 gaps.

Also open: wrap for vertical text, initials and markers; other stories' wrap around
anchored items; note-body variables; tables and the other item 9 gaps listed in
the roadmap. Production INDD remains gated on Phase 0.

## Working constraints

- Use `make` for builds and `CARGO_INCREMENTAL=0`. Implement a substantial
  coherent batch before compiling/testing it. Keep source/tests fixed during the
  final sweep and record any correction made during it.
- Test locally; do not query or wait for CI. Preserve real failures until resolved.
- One gesture is one undo step. Read existing documented exceptions before editing.
- Keep raster documents and `ToolPlugin` separate from Design Mode.
- Route every UI string through i18n and update all 150 catalogs. Existing English
  placeholders are deliberate. Follow AGENTS.md if adding locales.
- Never read Adobe headers or decompile binaries. Follow the roadmap's INDD spike
  gate and documented public-source permissions; IDML work uses public XML.
- Keep UI compact: icon toolbars, concise rows and fields exposed on demand.
- Review the actual development app with the feature enabled, using isolated
  configuration (`SCHIST_FEATURE_FLAGS={"design-mode":true}`); never use the
  user's installed non-development copy.
- Existing authorization covers committing, pushing and updating draft PR #195.
  Inspect `git status --short` and use `git add -A`, including new files.

## This Windows machine

- `make`, `zip` and Poppler are installed privately under
  `C:\Users\Administrator\.schist-tools`. Source `env.sh` there in Git Bash: it
  prepends them to PATH, sets `CARGO_INCREMENTAL=0`, and sets `PYTHONUTF8=1`,
  without which `tools/check-i18n.py` fails decoding `locales.tsv` as cp1252.
  `python3` is a shim to the installed Python, which has Pillow.
- Git's system config sets `core.autocrlf=true`. The repository is LF; create any
  worktree with `git -c core.autocrlf=false worktree add …` and check staged files
  with `git -c core.autocrlf=false diff --cached --check`. Git Bash `grep` hides
  CR bytes; detect CRLF with Python.
- Never point two checkouts at one `CARGO_TARGET_DIR`. Cargo hashes path crates
  relative to the workspace root, so one checkout reuses the other's artifacts.
- Native review: the shell runs as SYSTEM in session 0. Attach the Administrator
  session (`tscon 2 /dest:console`), start the `afworker` scheduled task, and send
  PowerShell to it with `C:\afprobe\af.sh`. The console is 800×600. Helpers for
  Schist are in `C:\afprobe\schist-qa.ps1`; `target/design-ui/qa-win/launch.ps1`
  starts a binary with Design enabled and HOME, USERPROFILE, APPDATA, LOCALAPPDATA
  and XDG directories isolated under `target/design-ui/qa-win/`.
- The console desktop can be shared with other applications being driven at the
  same time, and the app handles a click only when the next input event arrives.
  `C:\afprobe\wrap-qa.ps1` withholds input unless Schist is the responding
  foreground window and nudges the pointer after each action;
  `C:\afprobe\passive-shot.ps1` opens a document and captures Schist's own window
  with PrintWindow without sending any input. Prefer passive captures of documents
  carrying the state under review. PowerShell's built-in `Type` alias shadows
  functions of that name. GPUI reports Space on Windows without its character;
  text entry must use `schist_ui::typed_text`.

## Verification at this checkpoint

The first table batch passes all 16 roadmap targets, headless library wasm,
shared UI, formatting, whitespace and the debug app build (2,225 distinct passing
Rust tests); the i18n audit's finding on a long new message was fixed by
shortening it, and that check re-passed. All 43 proofs are byte-identical to the
previous checkpoint. Details are in Roadmap / Handoff. The two macOS-only editor tests were not compiled here; the next macOS
run should include them.

## Temporary files

Task logs and evidence are under the Git Bash `/tmp` (`C:\Users\Administrator\AppData\Local\Temp`):
`schist-validate/` (`aaad6dc0` logs, proofs, hashes, native screenshots),
`schist-sweep/` to `schist-sweep16/` (batch sweep logs), `schist-proofs-b3/` to
`schist-proofs-b12/` (proof PDFs per batch), `schist-tables-research/` (table
sample PDFs, generators and `pdfops2.py`), `schist-b3/` to `schist-b6/`
(review-document generators and logs), `schist-wrap-research/` (text-wrap references), `schist-anchored-research/` (the
anchored sample's PDF, inputs and measuring scripts `pdfops.py`/`measure.py`) and
`schist-variable-research/` (reacquired public specification and paged-media
reference). They are not needed to continue; regenerate proofs with make and
reacquire references from the pinned URLs in `docs/idml-format.md`.

## Prompt for the next agent

> Continue Schist on branch `design-tab-leaders`, draft PR #195:
> https://github.com/Infrawrench/schist/pull/195. Fetch the latest branch head.
> Read `AGENTS.md` first, then `docs/roadmap.md` in full, then
> `docs/design-handoff.md`. The roadmap is the plan; do not re-plan from scratch.
> The latest commit composes tables (first phase); its full sweep and native
> review passed on Windows. Continue Phase 3 item 9 in roadmap
> order with the remaining gaps listed in the handoff. Implement substantial coherent batches
> before compiling/testing; use make and `CARGO_INCREMENTAL=0`, test locally, and
> do not query or wait for CI. Keep Design's LayoutDocument and tools separate from
> raster Document/ToolPlugin; keep the feature flag false by default; preserve
> one-gesture undo and all-locale i18n coverage. Keep the UI compact with icons
> and controls shown when needed. Use the actual development app with
> `SCHIST_FEATURE_FLAGS='{"design-mode":true}'` and isolated configuration, not my
> installed non-development copy. Never read Adobe headers or decompile binaries;
> follow the roadmap's public-source rules and Phase 0 gate before any production
> INDD work. Use git add -A for commits, push the branch and update the existing
> draft PR.
