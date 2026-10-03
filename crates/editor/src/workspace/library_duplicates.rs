//! Gallery ▸ Find Duplicates: the Similar photos review, run over every
//! watched folder with byte-identical groups first, a suggested keeper, and
//! group actions (reject the rest, move the rest to the trash, keep all).
use super::library_similar::{ReviewKind, SimilarReview};
use super::*;
use schist_gallery::culling::{self, CullEdit, CullFlag};
use schist_gallery::duplicates::{self, KeepFacts, Refusal, TrashRequest};
use schist_gallery::similar::{Cache, Choice, Decisions, Group, Photo, Stamp};
use schist_i18n::{t, tn};
use schist_ui::Button;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize};

/// What a finished scan hands back to the UI thread.
struct Outcome {
    photos: Vec<Photo>,
    groups: Vec<Group>,
    exact: Vec<bool>,
    sha: HashMap<usize, duplicates::Sha256Digest>,
    fresh: Vec<(PathBuf, schist_gallery::FileDigest)>,
    failed: usize,
    near_limited: bool,
    cancelled: bool,
    decisions: std::io::Result<Decisions>,
}

fn keep_facts(photo: &Photo) -> KeepFacts {
    let attachments = duplicates::attachments(&photo.path);
    let xmp = attachments
        .iter()
        .any(|p| p.extension().is_some_and(|e| e == "xmp"));
    KeepFacts {
        path: photo.path.clone(),
        pixels: image::image_dimensions(&photo.path)
            .map(|(w, h)| u64::from(w) * u64::from(h))
            .unwrap_or(0),
        edited: attachments.len() > usize::from(xmp),
        sidecar: xmp,
        when: photo.captured.unwrap_or(photo.stamp.seconds as i64),
        bytes: photo.stamp.bytes,
    }
}

/// The scan itself, off the UI thread. Exact groups come first, then
/// near-duplicates; each group starts with its suggested keeper.
fn scan(
    paths: Vec<PathBuf>,
    known: FxHashMap<PathBuf, schist_gallery::FileDigest>,
    threshold: u32,
    cancel: &AtomicBool,
    progress: &AtomicUsize,
    total: &AtomicUsize,
) -> Outcome {
    let cancelled = |photos| Outcome {
        photos,
        groups: Vec::new(),
        exact: Vec::new(),
        sha: HashMap::new(),
        fresh: Vec::new(),
        failed: 0,
        near_limited: false,
        cancelled: true,
        decisions: Ok(Decisions::default()),
    };
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            return cancelled(Vec::new());
        }
        if let Some(stamp) = Stamp::read(&path) {
            files.push((path, stamp));
        }
    }
    let colliding: usize = duplicates::size_collisions(&files)
        .iter()
        .map(Vec::len)
        .sum();
    let stills: Vec<PathBuf> = files
        .iter()
        .map(|(p, _)| p.clone())
        .filter(|p| !schist_gallery::is_video(p))
        .collect();
    let near_limited = stills.len() > 10_000;
    total.store(colliding + stills.len().min(10_000), Ordering::Relaxed);
    let exact = duplicates::exact_groups(&files, |p| known.get(p).copied(), cancel, progress);
    if cancel.load(Ordering::Relaxed) {
        return cancelled(Vec::new());
    }
    let cache_path = schist_gallery::state_dir().map(|p| p.join("schist/similar-v1.json"));
    let mut cache = cache_path.as_deref().map(Cache::load).unwrap_or_default();
    let mut photos = cache.scan(
        &stills,
        cancel,
        progress,
        super::library_similar::signature_image,
        super::library_similar::capture_seconds,
    );
    if let Some(path) = cache_path {
        if let Err(error) = cache.save(&path) {
            log::warn!("similar cache write failed: {error}");
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return cancelled(Vec::new());
    }
    let failed = exact.failed
        + photos
            .iter()
            .filter(|p| p.signature.is_none() && !schist_gallery::is_video(&p.path))
            .count();
    let mut index: HashMap<PathBuf, usize> = photos
        .iter()
        .enumerate()
        .map(|(i, p)| (p.path.clone(), i))
        .collect();
    let mut sha = HashMap::new();
    let mut exact_groups: Vec<Vec<usize>> = Vec::new();
    for group in &exact.groups {
        let mut members: Vec<usize> = group
            .iter()
            .map(|&file| {
                let (path, stamp) = &files[file];
                let at = *index.entry(path.clone()).or_insert_with(|| {
                    // Videos and photos past the near-duplicate limit.
                    photos.push(Photo {
                        path: path.clone(),
                        stamp: stamp.clone(),
                        signature: None,
                        captured: None,
                    });
                    photos.len() - 1
                });
                if let Some(digest) = exact.digests.get(&file) {
                    sha.insert(at, *digest);
                }
                at
            })
            .collect();
        // A signed member represents the copies in the visual pass.
        members.sort_by_key(|&i| photos[i].signature.is_none());
        exact_groups.push(members);
    }
    let near = duplicates::near_groups(&photos, &exact_groups, threshold, cancel);
    if cancel.load(Ordering::Relaxed) {
        return cancelled(Vec::new());
    }
    let decisions = super::library_similar::decision_path()
        .ok_or_else(|| std::io::Error::other(t("common.not_available")))
        .and_then(|p| Decisions::load(&p));
    let mut groups = Vec::new();
    let mut kinds = Vec::new();
    for (members, is_exact) in exact_groups
        .into_iter()
        .map(|g| (g, true))
        .chain(near.into_iter().map(|g| (g, false)))
    {
        // "Keep all" is remembered until one of the files changes.
        if let Ok(decisions) = &decisions {
            if members
                .iter()
                .all(|&i| decisions.get(&photos[i]) == Some(Choice::Keep))
            {
                continue;
            }
        }
        let facts: Vec<KeepFacts> = members.iter().map(|&i| keep_facts(&photos[i])).collect();
        let keep = duplicates::suggest_keep(&facts);
        let mut members = members;
        members.swap(0, keep);
        groups.push(Group { photos: members });
        kinds.push(is_exact);
    }
    Outcome {
        photos,
        groups,
        exact: kinds,
        sha,
        fresh: exact.fresh,
        failed,
        near_limited,
        cancelled: false,
        decisions,
    }
}

fn refusal_text(refusal: &Refusal) -> &'static str {
    match refusal {
        Refusal::HasAttachments => "library.duplicates.refused_edited",
        Refusal::Trash(_) => "library.duplicates.trash_failed",
        _ => "library.similar.stale",
    }
}

impl Workspace {
    /// Gallery ▸ Find Duplicates: the review panel, scanning every watched
    /// folder regardless of the current filters.
    pub(crate) fn open_duplicate_finder(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.library.similar;
        if state.kind != ReviewKind::Duplicates {
            if state.running {
                // A Similar photos scan still running must not fill the
                // duplicate view; it ends as cancelled and Refresh scans.
                state.cancel.store(true, Ordering::Relaxed);
                state.kind = ReviewKind::Duplicates;
                self.show_review(cx);
                return;
            }
            state.kind = ReviewKind::Duplicates;
            state.photos.clear();
            state.groups.clear();
            state.exact.clear();
        }
        self.show_review(cx);
    }

    pub(super) fn scan_duplicates(&mut self, cx: &mut Context<Self>) {
        if self.library.similar.running {
            return;
        }
        let mut paths: Vec<PathBuf> = self
            .library
            .sections
            .iter()
            .flat_map(|s| s.entries.iter())
            .map(|e| e.path.clone())
            .filter(|p| schist_gallery::variants::original(p).is_none())
            .collect();
        paths.sort();
        paths.dedup();
        let known = self.library.digests.clone();
        let state = &mut self.library.similar;
        state.begin_scan(paths.len());
        let (cancel, progress, total, threshold) = (
            state.cancel.clone(),
            state.progress.clone(),
            state.total.clone(),
            state.threshold,
        );
        self.similar_progress_ticker(cx);
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { scan(paths, known, threshold, &cancel, &progress, &total) })
                .await;
            this.update(cx, |ws, cx| {
                ws.library.record_digests(outcome.fresh);
                let state = &mut ws.library.similar;
                state.running = false;
                state.cancelled = outcome.cancelled;
                state.failed = outcome.failed;
                state.near_limited = outcome.near_limited;
                if !outcome.cancelled {
                    state.photos = outcome.photos;
                    state.groups = outcome.groups;
                    state.exact = outcome.exact;
                    state.sha = outcome.sha;
                }
                state.group = 0;
                state.candidate = 1;
                state.finish_decisions(outcome.decisions);
                ws.load_similar_pair(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn duplicate_group(&self) -> Option<(usize, Vec<usize>)> {
        let state = &self.library.similar;
        state
            .groups
            .get(state.group)
            .map(|g| (state.group, g.photos.clone()))
    }

    /// Make a member the one to keep: it moves to the left pane.
    pub(super) fn duplicates_set_keeper(&mut self, index: usize, cx: &mut Context<Self>) {
        let state = &mut self.library.similar;
        let Some(group) = state.groups.get_mut(state.group) else {
            return;
        };
        if let Some(at) = group.photos.iter().position(|i| *i == index) {
            group.photos.swap(0, at);
            state.candidate = if at == 0 { 1 } else { at };
            state.trash_armed = None;
            self.load_similar_pair(cx);
        }
        cx.notify();
    }

    /// Flag every member but the keeper as Reject (the culling flag, as the
    /// X key does); files stay where they are.
    fn duplicates_reject_others(&mut self, cx: &mut Context<Self>) {
        let Some((_, members)) = self.duplicate_group() else {
            return;
        };
        let paths: Vec<PathBuf> = members[1..]
            .iter()
            .map(|&i| self.library.similar.photos[i].path.clone())
            .collect();
        let previous = self.library.culling.clone();
        culling::edit(
            &mut self.library.culling,
            &paths,
            CullEdit::Flag(CullFlag::Reject),
        );
        if let Err(error) = self.library.save_checked() {
            self.library.culling = previous;
            self.library.similar.error =
                Some(schist_i18n::tf!("library.ops.save_failed", error = error));
        } else {
            self.library.culling_changed();
            self.library.similar.error =
                Some(tn("library.duplicates.n_rejected", paths.len() as u64));
            self.duplicates_next_group(false, cx);
        }
        cx.notify();
    }

    /// Remember every member as kept and take the group off the list.
    fn duplicates_keep_all(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.library.similar;
        if !state.decisions_loaded {
            return;
        }
        let Some(group) = state.groups.get(state.group).cloned() else {
            return;
        };
        let mut next = state.decisions.clone();
        let all_set = group
            .photos
            .iter()
            .all(|&i| next.set(&state.photos, &group, i, Some(Choice::Keep)));
        if !all_set {
            state.error = Some(t("library.similar.stale").into());
            cx.notify();
            return;
        }
        match super::library_similar::decision_path()
            .ok_or_else(|| std::io::Error::other(t("common.not_available")))
            .and_then(|p| next.save(&p))
        {
            Ok(()) => {
                state.decisions = next;
                state.error = None;
                self.duplicates_next_group(true, cx);
            }
            Err(error) => state.error = Some(super::library_similar::review_io_error(error)),
        }
        cx.notify();
    }

    /// Move on from the current group, dropping it from the list if done.
    fn duplicates_next_group(&mut self, remove: bool, cx: &mut Context<Self>) {
        let state = &mut self.library.similar;
        state.trash_armed = None;
        if remove && state.group < state.groups.len() {
            state.groups.remove(state.group);
            state.exact.remove(state.group);
        } else {
            state.group += 1;
        }
        if state.group >= state.groups.len() {
            state.group = state.groups.len().saturating_sub(1);
        }
        state.candidate = 1;
        self.load_similar_pair(cx);
    }

    /// Two clicks: the first arms, the second sends every member but the
    /// keeper to the platform trash after re-verifying each one.
    fn duplicates_trash_others(&mut self, cx: &mut Context<Self>) {
        let Some((group_index, members)) = self.duplicate_group() else {
            return;
        };
        let state = &mut self.library.similar;
        if state.trash_armed != Some(group_index) {
            state.trash_armed = Some(group_index);
            cx.notify();
            return;
        }
        state.trash_armed = None;
        state.running = true;
        let exact = state.exact.get(group_index).copied().unwrap_or(false);
        let keeper = &state.photos[members[0]];
        let requests: Vec<(usize, TrashRequest)> = members[1..]
            .iter()
            .map(|&i| {
                let victim = &state.photos[i];
                (
                    i,
                    TrashRequest {
                        victim: victim.path.clone(),
                        victim_stamp: victim.stamp.clone(),
                        keeper: keeper.path.clone(),
                        keeper_stamp: keeper.stamp.clone(),
                        sha256: if exact {
                            state.sha.get(&i).copied()
                        } else {
                            None
                        },
                    },
                )
            })
            // An exact group without a recorded hash cannot be re-verified.
            .filter(|(_, r)| !exact || r.sha256.is_some())
            .collect();
        // Its own flag: closing the review cancels scans, never a trash run
        // halfway through re-verifying.
        let cancel = AtomicBool::new(false);
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    requests
                        .into_iter()
                        .map(|(i, request)| {
                            let result = duplicates::trash_checked(
                                &request,
                                &cancel,
                                duplicates::system_trash,
                            );
                            if let Err(refusal) = &result {
                                log::warn!(
                                    "duplicate {} not trashed: {refusal:?}",
                                    request.victim.display()
                                );
                            }
                            (i, result)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |ws, cx| {
                let state = &mut ws.library.similar;
                state.running = false;
                let trashed: Vec<usize> = results
                    .iter()
                    .filter(|(_, r)| r.is_ok())
                    .map(|(i, _)| *i)
                    .collect();
                let refusal = results.iter().find_map(|(_, r)| r.as_ref().err());
                let mut message = tn("library.duplicates.n_trashed", trashed.len() as u64);
                if let Some(refusal) = refusal {
                    let refused = results.len() - trashed.len();
                    message = format!(
                        "{message} · {} {refused}: {}",
                        t("common.failed"),
                        t(refusal_text(refusal))
                    );
                }
                state.error = Some(message);
                if let Some(group) = state.groups.get_mut(group_index) {
                    group.photos.retain(|i| !trashed.contains(i));
                    let done = group.photos.len() < 2;
                    if state.group == group_index {
                        state.candidate = 1;
                        if done {
                            ws.duplicates_next_group(true, cx);
                        } else {
                            ws.load_similar_pair(cx);
                        }
                    }
                }
                if !trashed.is_empty() {
                    ws.library_rescan(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The synchronized two-up comparison of keeper and candidate; closing
    /// it comes back to this review.
    fn duplicates_compare(&mut self, cx: &mut Context<Self>) {
        let state = &self.library.similar;
        let Some(group) = state.groups.get(state.group) else {
            return;
        };
        let pair = [
            state.photos[group.photos[0]].path.clone(),
            state.photos[group.photos[state.candidate]].path.clone(),
        ];
        if pair.iter().any(|p| schist_gallery::is_video(p)) {
            return;
        }
        self.library.selected = pair.to_vec();
        self.open_culling_compare(cx);
        self.library.similar.resume = true;
    }
}

/// The duplicate finder's group row: what kind of group this is and the
/// actions on all of it.
pub(super) fn group_actions(state: &SimilarReview, cx: &mut Context<Workspace>) -> gpui::Div {
    let Some(group) = state.groups.get(state.group) else {
        return div();
    };
    let others = group.photos.len().saturating_sub(1) as u64;
    let exact = state.exact.get(state.group).copied().unwrap_or(false);
    let busy = state.running;
    let armed = state.trash_armed == Some(state.group);
    let still_pair = group
        .photos
        .iter()
        .take(1)
        .chain(group.photos.get(state.candidate))
        .all(|&i| !schist_gallery::is_video(&state.photos[i].path));
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .child(
            div()
                .px_2()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(t(if exact {
                    "library.duplicates.exact"
                } else {
                    "library.duplicates.near"
                })),
        )
        .child(div().flex_grow())
        .child(
            Button::new("dupes-compare", t("library.duplicates.compare"))
                .ghost()
                .px_2()
                .disabled(busy || !still_pair)
                .on_click(cx.listener(|ws, _, _, cx| ws.duplicates_compare(cx))),
        )
        .child(
            Button::new("dupes-keep-all", t("library.duplicates.keep_all"))
                .ghost()
                .px_2()
                .disabled(busy || !state.decisions_loaded)
                .on_click(cx.listener(|ws, _, _, cx| ws.duplicates_keep_all(cx))),
        )
        .child(
            Button::new("dupes-reject", t("library.duplicates.reject_others"))
                .ghost()
                .px_2()
                .disabled(busy)
                .on_click(cx.listener(|ws, _, _, cx| ws.duplicates_reject_others(cx))),
        )
        .child(
            Button::new(
                "dupes-trash",
                if armed {
                    tn("library.duplicates.confirm_trash", others)
                } else {
                    tn("library.duplicates.trash_others", others)
                },
            )
            .ghost()
            .px_2()
            .active(armed)
            .disabled(busy)
            .on_click(cx.listener(|ws, _, _, cx| ws.duplicates_trash_others(cx))),
        )
}
