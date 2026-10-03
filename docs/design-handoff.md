# Design Mode handoff — 2026-10-03

Read `AGENTS.md`, then `docs/roadmap.md` in full. The roadmap is authoritative;
do not recreate the plan from the original conversation. This handoff supplements
its latest checkpoint for work on another machine.

## Publication

Branch: `design-tab-leaders`. Existing draft PR:
https://github.com/Infrawrench/schist/pull/195

The previous published checkpoint is `b0cdef3c` (custom-variable display).
This handoff accompanies the custom-variable authoring commit. The user explicitly
stopped validation on this machine to continue elsewhere; this is a partial
checkpoint. Fetch the branch head rather than checking out the older display commit.

## Current work

Phase 3 items 1–8 are implemented. Item 9, output and interchange integration,
remains open. Design Mode is a separate `LayoutDocument` editor; it stays disabled
by default behind the `design-mode` feature flag.

The current batch adds a compact Text Variables window under Type and the Stories
toolbar. Draft name/value fields appear only for New/Edit. Save changes the shared
resource once; insertion and explicit instance removal target a captured Unicode
cursor. Unused definitions can be deleted. Stale definitions, stories and sessions
are rejected. IDs reserve unresolved typed references, including note bodies;
opaque XML remains the codec's responsibility. Eleven keys were added to all 150
existing locale catalogs. No locale was added. Canvas cursors still denote source
byte boundaries; coincident zero-width instances are distinguished by the manager
rows, not separate byte positions inside the displayed values. Insertion captures
the canvas cursor; Story Editor and footnote-body cursor integration remain open.
The new window has not yet been reviewed in the running native app.

Key files:

- `crates/layout/src/text_variables.rs`: model operations and source cursor guards.
- `crates/layout/src/history.rs`, `edit.rs`: reversible definition edits.
- `crates/editor/src/design/text_variables.rs`: compact modeless authoring window.
- `crates/codec-idml/src/custom_text_codec.rs`, `text_variable_codec.rs`: native
  definitions, guarded identities and inert recovery data.
- `crates/layout/tests/text_variable_authoring.rs` and
  `crates/codec-idml/tests/text_variables.rs`: undo, source, identity and save properties.

## Continue in roadmap order

Non-custom variable evaluation/output is still missing. The current literal
projection resolves a story before frame selection; page/section-dependent values
must not be guessed from the first frame. Typed definitions and guarded native
interchange precede their composition and UI. The public IDML specification names
`PageNumberVariablePreference` with `TextBefore`, `Format`, `TextAfter` and `Scope`.
The public native variable reference described in `docs/idml-format.md` uses five
pages with restarted numbering; the document's final label is 3, not a count of 5.

Other explicit gaps include variables with active initial/nested rules, note-body
variables, page/section/indent markers, further structured-story composition,
advanced object behavior and independent native application agreement. The roadmap
and `docs/idml-format.md` distinguish implemented subsets from retained/diagnosed
settings. Production INDD remains gated on the Phase 0 research decision.

## Working constraints

- Use `make` for builds and `CARGO_INCREMENTAL=0` for the editor. Implement a
  substantial coherent batch before compiling/testing it; do not build after
  every small change. Keep source/tests fixed during validation.
- Test locally; do not query or wait for CI. Preserve real failures until resolved.
- One gesture is one undo step. Read existing documented exceptions before editing.
- Keep raster documents and `ToolPlugin` separate from Design Mode.
- Route every UI string through i18n and update all 150 catalogs. Existing English
  placeholders are deliberate. Follow AGENTS.md if adding locales.
- Never read Adobe headers or decompile binaries. Follow the roadmap's INDD spike
  gate and documented public-source permissions; IDML work uses public XML.
- Keep UI compact: icon toolbars, concise rows and fields exposed on demand.
- Review the actual development app with the feature enabled, using an isolated
  configuration (`SCHIST_FEATURE_FLAGS={"design-mode":true}`); do not operate on the user's installed non-development copy.
- Existing authorization covers committing, pushing and updating draft PR #195.
  Inspect `git status --short` and use `git add -A`, including new files.

## Validation stopped by user — resume here first

No more validation was run after the user's stop. The active output sweep and its
children were terminated; no task compiler/test process remained. Existing logs
were read to record this summary before cleanup.

| Status | Targets / work |
| --- | --- |
| Passed | `check-layout`, `check-idml`, `check-design`, `check-i18n` |
| Passed | `lint-layout`, `lint-text-directions`, `lint-idml`, `lint-design`, `lint-all` |
| Passed | `check-layered-codecs-app`, `check-app-web`, `check-separation` |
| Interrupted | `check-design-output` — terminated at user request, not a pass |
| Not run for this batch | `check-editable-interchange`, `lint-editable-interchange`, `check-library-wasm`, separate shared UI tests |
| Pending final checks | Formatting/whitespace, full distinct test count, aggregate proof comparison |
| Pending native review | `make app PROFILE=debug`, then run the actual development app with Design enabled and review the new window |

The completed package logs contain **1,863 distinct passing Rust tests**: layout
512, text engine 151, editor 436, app actions 12, app settings 26, i18n 28, IDML
360, core 120 and separation 218. Four browser i18n checks and eight Python audits
also passed. Nine new properties (six layout, three IDML) are included. These
counts exclude unfinished targets; do not report the expected full total as a pass.

The first editor compile failed on a missing closing delimiter in the new window.
The delimiter was fixed; `cargo fmt --all` and `git diff --check` passed, then the
complete Design rerun passed. No tests were weakened or ignored. The final
format/whitespace recheck was deferred with the remaining validation. Existing
future-compatibility warnings for `block 0.1.6` and `proc-macro-error2 2.0.1` remain.

The previous full checkpoint at `b0cdef3c` passed 2,086 distinct Rust tests, four
browser checks and eight Python audits, with one existing shared UI doc example
ignored. Its native build/review and proof comparisons do not validate this new
window. The local development binary was not refreshed for this batch.

Resume only the pending work unless source changes or platform differences justify
repeating passed targets. Use the repository Makefile; for the separate shared UI
tests, add a temporary make fragment as previous checkpoints did:

```sh
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
make check-design-output check-editable-interchange lint-editable-interchange
CARGO_PROFILE_DEV_DEBUG=0 make check-library-wasm
qa_makefile=$(mktemp)
printf 'check-handoff-shared-ui:\n\t$(CARGO) test -p schist-ui\n' > "$qa_makefile"
make -f Makefile -f "$qa_makefile" check-handoff-shared-ui
rm -f "$qa_makefile"
cargo fmt --all -- --check
git diff --check
make app PROFILE=debug
```

Record each command's exit status and stop to investigate real failures. Count
unique package tests rather than summing overlapping make targets. Output proofs
can be regenerated by `check-design-output`; the old local hash baseline was
removed during requested cleanup, so do not claim an unchanged-proof comparison
without reconstructing a baseline from the previous commit in an isolated checkout.
`lint-all` already passed workspace clippy with all targets, and
`check-layered-codecs-app` already passed the `schist-app` compile check.

In the actual development app, use isolated configuration and
`SCHIST_FEATURE_FLAGS='{"design-mode":true}'`. Review Type → Text Variables and the
Stories toolbar; New/Edit/Cancel/Save; shared-instance updates and undo/redo;
Unicode cursor capture/recapture; coincident instance removal; used-definition
delete refusal; stale-story/session refusal; and IDML save/reopen. Check the compact
layout at normal scale. Do not use the user's installed non-development app.

## Temporary files

Removed 3,236 task-created entries under `/tmp/schist*`: logs, scripts, proof
PDFs/renders, temporary PR drafts
and research downloads, at the user's request. The isolated
`target/design-ui/Schist Roadmap QA.app`, its configuration/state and task QA
fixtures were also removed. Earlier cleanup reclaimed about 2.1 GB of inactive
incremental caches. Tracked fixtures, normal build outputs and the user's
`Schist Dev.app`, configuration/state and existing `review.idml` were preserved.

Historical `/tmp/schist-*` references in the roadmap and format notes describe
previous evidence. Regenerate proofs with make and reacquire public references
from the pinned URLs in those documents. No evidence depends on transferring a
local temporary file to the next machine.

## Prompt for the next agent

> Continue Schist on branch `design-tab-leaders`, draft PR #195:
> https://github.com/Infrawrench/schist/pull/195. Fetch the latest branch head.
> Read `AGENTS.md` first, then `docs/roadmap.md` in full, then
> `docs/design-handoff.md`. The roadmap is the plan; do not re-plan from scratch.
> The latest commit adds compact custom text-variable authoring. I explicitly
> stopped validation to move machines: 12 make targets passed, output validation
> was interrupted, and the remaining checks plus native build/UI review are
> listed in the handoff. Resume those first; do not claim a full sweep or native
> UI review already passed. Then continue Phase 3 item 9 in roadmap order.
> Implement substantial coherent batches before compiling/testing; use make and
> `CARGO_INCREMENTAL=0`, test locally, and do not query or wait for CI. Keep
> Design's LayoutDocument and tools separate from raster Document/ToolPlugin;
> keep the feature flag false by default; preserve one-gesture undo and all-locale
> i18n coverage. Keep the UI compact with icons and controls shown when needed.
> Use the actual development app with `SCHIST_FEATURE_FLAGS='{"design-mode":true}'`
> and isolated configuration, not my installed non-development copy. Never read
> Adobe headers or decompile binaries; follow the roadmap's public-source rules
> and Phase 0 gate before any production INDD work. Use git add -A for commits,
> push the branch and update the existing draft PR. Task temporary files were
> cleaned up; regenerate them from tracked sources and pinned public references.
