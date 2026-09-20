# Cloud catch-up: September 18–20, 2026

Reviewed all 35 commits on the app branch during this UTC interval, through `865ab555` (including squash-merged PRs and direct maintenance commits). The service checkout is `/home/infrawrench-agent/projects/schist-cloud`; the relative path in the request did not exist from this worktree.

## Implementation

The app now discovers five additive `gallery_features`: metadata, culling, review, versions and workflows. It connects cloud-backed metadata/XMP editing, rating/flag/label decisions, comparison, similar/burst review, and retained versions to authenticated workspace APIs. Saved brushes, recorded actions and export recipes sync between devices with account-scoped three-way merge checkpoints. Simultaneous edits keep both copies. Local export destination paths stay on each device. Cloud-selected actions and recipes download sources and upload separate results into their source cloud folders.

The cloud service adds account-owned version, workflow and signature tables; atomic optimistic catalogue mutations; signed preview/XMP/history downloads; workflow compare-and-swap writes; and website photo metadata/rating/history controls. Its bundled document worker is repackaged from this app revision so recent document and codec features are available during cloud validation/export.

API details and limits are in [the cloud protocol extension](https://github.com/Infrawrench/schist-cloud/blob/main/docs/gallery-sync.md). All mutations remain authenticated, account-scoped and idempotent. Catalogue revisions are separate from content revisions, preserving editing leases and moderation state for metadata-only changes.

## Commit coverage

| Commit / PR | Change | Cloud disposition |
| --- | --- | --- |
| `865ab555` / #164 | Add editable spot ink channels and PSD separations (#164) | Updated shared engine preserves spot-ink metadata/tiles and PSD separations. |
| `bb577c5c` / #162 | Add photo ratings, flags and synchronized comparison (#162) | New catalogue culling API, cloud grid badges/filters, synchronized cloud comparison. |
| `3d2ae1fe` / #156 | Add similar-photo and capture-burst review to the gallery (#156) | New persistent original-photo signatures and reversible cloud review decisions; shared visual/burst grouping in the app. |
| `0afdaa8e` / #161 | Add editable photo metadata and portable XMP sidecars (#161) | New atomic metadata API, revision-bound XMP downloads, cloud app dialog and website editor. |
| `b6130243` / #163 | Add symmetry painting and seamless tile preview (#163) | Symmetry/tile preview run in the editor; resulting document pixels and layers use existing cloud document updates. |
| `1f50c31b` / #158 | Keep Affinity text and curves editable and expand PSD typography (#158) | Updated codecs preserve supported editable Affinity text/curves and PSD typography. |
| `1fa303e3` / #160 | Add photo alignment, focus stacking, bracketed HDR and panoramas (#160) | Merge tools operate on open documents, including downloaded cloud documents; resulting new documents can use the existing Save to Cloud upload. No separate remote compute API is needed. |
| `f1ad562f` / #157 | Import brush tips and packs with pressure opacity and pen tilt (#157) | Imported brush tips/packs remain part of synced brush presets; pressure/tilt are handled in the client. |
| `980c79b5` / #159 | Replace Schist Cloud pagination with infinite scrolling (#159) | Cloud integration already shipped: infinite scrolling remains in place; overlap deduplication now also compares metadata revisions. |
| `4e538088` / #154 | Add on-canvas filter parameter controls (#154) | On-canvas controls run in the editor; filter parameter changes use the shared cloud document model. |
| `7aa3e3f7` / #153 | Add native editable PSD text and smart-filter interchange (#153) | Updated codecs and document worker handle native text and smart-filter interchange. |
| `1652fd3e` / #152 | Add bidirectional and vertical text editing (#152) | Text layout/editing run in the editor; supported typography persists through the updated worker. |
| `11b156d3` / #151 | Extend recorded actions with transforms, layer targets, RAW and filter stacks (#151) | Saved action library sync and cloud-selected batch replay using the existing runtime. |
| `e52fecbb` / #150 | Add editable embedded and linked smart objects (#150) | Updated shared model preserves embedded editable smart-object sources. Device-local linked file paths still require accessible sources or embedding. |
| `e961af73` / #149 | Add saved brush presets, textured tips and stroke stabilization (#149) | Saved brush library sync, including embedded texture tips and preset parameters. |
| `cd90fbc8` / #148 | Preserve editable filter stacks through moves and transforms (#148) | Updated shared engine preserves live filter stacks through transforms. |
| `19b28518` / #146 | Simplify cloud people labels and add cloud icons (#146) | Cloud people labels/icons already integrated. |
| `cb2256b5` / #147 | Fix iOS app icon and share mobile logo generation (#147) | Platform icon/assets only; no remote data. |
| `eb720d3d` / #145 | Make filter selection searchable and tab the layer panel (#145) | Editor navigation only; no remote data. |
| `c33261ae` / #144 | Fix adjustment preview invalidation and working action selection (#144) | Client preview/action-selection fixes also apply when editing cloud documents. |
| `dd05e40c` / #141 | Add recordable actions with transactional and gallery replay (#141) | Saved action library sync and cloud-selected batch replay with separate uploaded outputs. |
| `30e60a61` / #142 | Add editable filter stacks with PSD and PSB persistence (#142) | Updated shared engine carries editable filter stacks through PSD/PSB and collaboration. |
| `285585ab` / #140 | Add visual gallery version history and restore as copy (#140) | Prospective cloud history, explicit checkpoints, previews, PSD download and restore as a new cloud asset. |
| `a97c7e29` / #143 | Fix cloud face viewer losing photos during refreshes (#143) | Cloud people refresh fix already integrated. |
| `3649edbb` / #139 | Add saved multi-output export recipes (#139) | Saved portable recipe sync and cloud-selected multi-output export uploads; destination paths stay local. |
| `6634b545` / #138 | Add mask refinement with edge cleanup and undoable output (#138) | Mask refinement runs in the editor and publishes raster/mask changes through existing document sync. |
| `80abd0ab` | chore: rename CLAUDE.md, remove hack in AGENTS.md | Release, packaging, feature-flag or repository instruction maintenance; no missing per-account data API. |
| `d3a3f8f7` | Reapply "chore: turn schist-cloud feature flag on" | Release, packaging, feature-flag or repository instruction maintenance; no missing per-account data API. |
| `8eddbde9` | chore: update AUR packages to v0.14.0 | Release, packaging, feature-flag or repository instruction maintenance; no missing per-account data API. |
| `a37ee6dd` | Bump version to 0.14.0 | Release, packaging, feature-flag or repository instruction maintenance; no missing per-account data API. |
| `589cc89d` | Decouple feature flag tests from shipping defaults | Release, packaging, feature-flag or repository instruction maintenance; no missing per-account data API. |
| `6aa4902b` | Revert "chore: turn schist-cloud feature flag on" | Release, packaging, feature-flag or repository instruction maintenance; no missing per-account data API. |
| `50f36bed` / #137 | Require libheif 1.23.4 and offer security updates for HEIC support (#137) | Existing cloud HEIF preparation already uses the secured runtime; new source bundle includes current codec checks. |
| `ebf1f725` / #136 | Fix browser viewport frame presentation during drags (#136) | Browser presentation fix is client-only and already applies to cloud-backed editing. |
| `15f60eff` / #135 | Expand GPU pipelines and asynchronous browser editing (#135) | GPU/browser execution is client-side; outputs continue through document sync. |

## Behavior and boundaries

- History starts when this service version is deployed. Automatic checkpoints coalesce to five minutes, explicit checkpoints save the current revision, and retention is 20 per photo / 512 MiB per account. Restoring creates a copy.
- Saved workflows sync after saves and approximately every 30 seconds while the connected app is idle. The app pauses applying remote libraries during open editing dialogs and recordings. Signing out leaves local personal workflows available.
- Actions and export recipes execute in the running app using the same local implementations. Cloud API uploads/downloads provide persistence; there is no unattended server compute queue.
- Local plugins, device-specific input settings, transient selections/undo state, and inaccessible filesystem links do not become portable merely by syncing a document. Linked content must be embedded or available on the target device.
- Original photo bytes remain unchanged when editing catalogue metadata; use the XMP endpoint for a portable sidecar. Review decisions are revision-bound and reversible. Compare previews are reduced to at most 4096 pixels.

## Verification

- `make check-cloud-gallery`: native cloud/editor all-target compilation.
- `make test-cloud-gallery`: 28 cloud-client tests, including legacy asset compatibility, conflict preservation, deletion merging and retry deduplication.
- `make check-document`: 16 document tests including spot-ink/filter persistence, PSD round trips and embedded smart sources.
- `make check-app-web`: browser app type-check.
- Cloud `make check` and `make test`, including PGlite migrations and real Rust-worker interoperability; gallery tests cover ownership, stale revisions, batch rollback, moderation/lease isolation, XMP, review protection, retention and workflow conflicts.
- Playwright exercises website ratings, metadata saves, retained unsaved drafts on conflicts, version controls, existing viewer navigation and People behavior.
