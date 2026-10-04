# Design Mode handoff — 2026-10-04

Read `AGENTS.md`, then `docs/roadmap.md` in full. The roadmap is authoritative;
do not recreate the plan from the original conversation. This handoff supplements
its latest checkpoint for work on another machine.

## Publication

Branch: `design-tab-leaders`. Existing draft PR:
https://github.com/Infrawrench/schist/pull/195

The latest commit adds last-page-number text variables and closes Design tool
windows with the main window. It follows `aaad6dc0` (custom-variable authoring),
whose interrupted validation and native review were completed first on this
Windows machine. Fetch the branch head.

## Current work

Phase 3 items 1–8 are implemented. Item 9, output and interchange integration,
remains open. Design Mode is a separate `LayoutDocument` editor; it stays disabled
by default behind the `design-mode` feature flag.

Text variables now have two typed kinds: literal custom text and the native
LastPageNumberType. A last-page value is evaluated for the pages a composition pass
can occupy: a parent instance uses its destination page; an ordinary thread renders
a section value only when every frame shares one section. Ambiguous scope, visible
section prefixes and unrendered formats (Kanji, full-width, leading zeros) stay
unrendered and diagnosed. The compact Text Variables window switches kinds with two
icons; last-page drafts show Text before/after with Format and Scope dropdowns.

Key files:

- `crates/layout/src/text_variables.rs`: model, validation and `last_page_value`.
- `crates/layout/src/compose.rs`, `footnote_composition.rs`: page context for projection.
- `crates/codec-idml/src/custom_text_codec.rs`: PageNumberVariablePreference subset.
- `crates/editor/src/design/text_variables.rs`: compact modeless window.
- `crates/app/src/lib.rs`: tool windows close with the main window.
- `crates/layout/tests/last_page_variables.rs`, `crates/codec-idml/tests/last_page_variables.rs`,
  `crates/separation/examples/support/text_variables.rs`: properties and proof.

## Continue in roadmap order

Next: chapter-number variables. The public specification's
ChapterNumberVariablePreference has TextBefore, Format and TextAfter; the value
comes from document chapter numbering, which is not yet modeled. Then file name
(needs the document path at output time), dates (output date is time-dependent;
format strings need a documented subset) and running headers (page-dependent
matching, like section scope). Active initial/nested-rule combinations, note-body
variables and Story Editor cursor integration remain open, as do the other item 9
gaps listed in the roadmap. Production INDD remains gated on Phase 0.

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

## Verification at this checkpoint

The pending `aaad6dc0` checks were completed from an isolated worktree before new
code compiled; that checkpoint has 2,095 distinct passing Rust tests and its 43
proof PDFs are byte-identical to `b0cdef3c`. The new batch passes the source-frozen
sweep of all 16 roadmap targets, shared UI, formatting and whitespace with 2,105
distinct passing Rust tests, four browser checks and eight Python audits. Details,
the one in-sweep test correction and native review results are in Roadmap /
Handoff. The two macOS-only editor tests were not compiled here; the next macOS
run should include them.

## Temporary files

Task logs and evidence are under the Git Bash `/tmp` (`C:\Users\Administrator\AppData\Local\Temp`):
`schist-validate/` (`aaad6dc0` logs, proofs, hashes, native screenshots),
`schist-sweep/` (batch sweep logs, proofs, hashes, native screenshots) and
`schist-variable-research/` (reacquired public specification and paged-media
reference). They are not needed to continue; regenerate proofs with make and
reacquire references from the pinned URLs in `docs/idml-format.md`.

## Prompt for the next agent

> Continue Schist on branch `design-tab-leaders`, draft PR #195:
> https://github.com/Infrawrench/schist/pull/195. Fetch the latest branch head.
> Read `AGENTS.md` first, then `docs/roadmap.md` in full, then
> `docs/design-handoff.md`. The roadmap is the plan; do not re-plan from scratch.
> The latest commit adds last-page-number text variables; its full sweep and
> native review passed on Windows. Continue Phase 3 item 9 in roadmap order,
> starting with chapter-number variables. Implement substantial coherent batches
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
