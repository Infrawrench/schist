//! Selection-aware metadata editing; batches apply only checked fields.
use super::*;
use gpui::StatefulInteractiveElement as _;
use schist_gallery::xmp;
use schist_i18n::{t, tf, tn};

const IDS: [&str; 6] = [
    "metadata-keywords",
    "metadata-caption",
    "metadata-copyright",
    "metadata-taken",
    "metadata-offset",
    "metadata-gps",
];
const LABELS: [&str; 6] = [
    "metadata.keywords",
    "metadata.caption",
    "metadata.copyright",
    "metadata.taken",
    "metadata.offset",
    "metadata.gps",
];
const EXAMPLES: [&str; 6] = [
    ";",
    "",
    "©",
    "2026-09-20T14:30:00+02:00",
    "3600",
    "51.5074, -0.1278",
];

pub(super) fn commit_field(modal: &mut Modal, id: &str, buffer: String) -> bool {
    let Some(index) = IDS.iter().position(|key| *key == id) else {
        return false;
    };
    if let Modal::MetadataEdit {
        values,
        enabled,
        busy,
        ..
    } = modal
    {
        if !*busy && values[index] != buffer {
            values[index] = buffer;
            enabled[index] = true;
            if index == 3 {
                enabled[4] = false;
            }
            if index == 4 {
                enabled[3] = false;
            }
        }
    }
    true
}

fn build_patch(values: &[String; 6], enabled: &[bool; 6]) -> Result<xmp::Patch, usize> {
    let mut patch = xmp::Patch::default();
    if enabled[0] {
        let mut words = Vec::new();
        for word in values[0]
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if !words.iter().any(|s| s == word) {
                words.push(word.to_string());
            }
        }
        patch.keywords = Some(words);
    }
    if enabled[1] {
        patch.caption = Some(values[1].clone());
    }
    if enabled[2] {
        patch.copyright = Some(values[2].clone());
    }
    if enabled[3] {
        let time = values[3].trim();
        patch.taken = Some(if time.is_empty() {
            String::new()
        } else {
            xmp::normalize_time(time).map_err(|_| 3usize)?
        });
    }
    if enabled[4] {
        if enabled[3] {
            return Err(4);
        }
        patch.offset_seconds = Some(values[4].trim().parse().map_err(|_| 4usize)?);
    }
    if enabled[5] {
        let value = values[5].trim();
        patch.gps = Some(if value.is_empty() {
            None
        } else {
            let (lat, lon) = value.split_once(',').ok_or(5usize)?;
            Some((
                lat.trim().parse::<f64>().map_err(|_| 5usize)?,
                lon.trim().parse::<f64>().map_err(|_| 5usize)?,
            ))
        });
        patch.validate().map_err(|_| 5usize)?;
    }
    patch.validate().map_err(|_| 1usize)?;
    Ok(patch)
}

impl Workspace {
    pub(super) fn open_metadata_editor(&mut self, photos: Vec<PathBuf>, cx: &mut Context<Self>) {
        let photos: Vec<_> = photos
            .into_iter()
            .filter(|p| !schist_gallery::is_video(p))
            .collect();
        if photos.is_empty() {
            return;
        }
        let mut values: [String; 6] = Default::default();
        let mut error = String::new();
        if photos.len() == 1 {
            match xmp::read(&photos[0]) {
                Ok(meta) => {
                    let effective = schist_gallery::photo_meta(&None, &photos[0]);
                    values[0] = meta.keywords.join("; ");
                    values[1] = meta.caption;
                    values[2] = meta.copyright;
                    values[3] = effective.taken.unwrap_or_default().replace(' ', "T");
                    values[5] = effective
                        .gps
                        .map(|(lat, lon)| format!("{lat}, {lon}"))
                        .unwrap_or_default();
                }
                Err(err) => {
                    log::warn!("cannot read metadata for {}: {err:#}", photos[0].display());
                    error = t("metadata.invalid").into();
                }
            }
        }
        self.library.metadata_edit_generation += 1;
        let id = self.library.metadata_edit_generation;
        self.open_modal(
            Modal::MetadataEdit {
                id,
                photos,
                values,
                enabled: [false; 6],
                error,
                busy: false,
            },
            cx,
        );
    }

    pub(super) fn save_gallery_metadata(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Some(Modal::MetadataEdit {
            id,
            photos,
            values,
            enabled,
            busy: false,
            ..
        }) = self.modal.clone()
        else {
            return;
        };
        let patch = match build_patch(&values, &enabled) {
            Ok(patch) => patch,
            Err(field) => {
                self.update_modal(|m| {
                    if let Modal::MetadataEdit { error, .. } = m {
                        *error = format!("{}: {}", t(LABELS[field]), t("metadata.invalid"));
                    }
                });
                cx.notify();
                return;
            }
        };
        if patch.is_empty() {
            return;
        }
        self.update_modal(|m| {
            if let Modal::MetadataEdit { busy, error, .. } = m {
                *busy = true;
                error.clear();
            }
        });
        self.status = t("common.saving").into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcomes = cx
                .background_executor()
                .spawn(async move { xmp::write_batch(&photos, &patch) })
                .await;
            this.update(cx, |ws, cx| {
                let total = outcomes.len();
                let failed: Vec<_> = outcomes
                    .into_iter()
                    .filter_map(|(path, result)| match result {
                        Ok(_) => None,
                        Err(err) => {
                            log::error!("metadata write failed for {}: {err:#}", path.display());
                            Some(path)
                        }
                    })
                    .collect();
                let count = total - failed.len();
                ws.status = if failed.is_empty() {
                    tn("library.batch.done", count as u64)
                } else {
                    tf!("library.batch.done_partial", n = count, total = total)
                }
                .into();
                // Retrying a partial batch must not shift successful photos twice.
                // A dismissed or replaced dialog is never resurrected by a task.
                ws.update_modal(|m| {
                    if let Modal::MetadataEdit {
                        id: current_id,
                        photos,
                        enabled,
                        error,
                        busy,
                        ..
                    } = m
                    {
                        if *busy && *current_id == id {
                            *busy = false;
                            *photos = failed.clone();
                            if failed.is_empty() {
                                *enabled = [false; 6];
                                *error = t("common.done").into();
                            } else {
                                *error = format!(
                                    "{}\n{}",
                                    t("metadata.invalid"),
                                    failed
                                        .iter()
                                        .map(|p| p.display().to_string())
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                );
                            }
                        }
                    }
                });
                ws.refresh_gallery_metadata(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Read XMP independently of thumbnail/embedding freshness. Refresh discovers
    /// edits and deletions from other applications, even with a warm index snapshot.
    pub(super) fn refresh_gallery_metadata(&mut self, cx: &mut Context<Self>) {
        self.library.metadata_generation += 1;
        let generation = self.library.metadata_generation;
        let entries: Vec<_> = self
            .library
            .sections
            .iter()
            .flat_map(|s| s.entries.iter())
            .cloned()
            .collect();
        cx.spawn(async move |this, cx| {
            let rows = cx
                .background_executor()
                .spawn(async move {
                    entries
                        .into_iter()
                        .filter(|e| !schist_gallery::is_video(&e.path))
                        .map(|entry| {
                            let cache = schist_gallery::thumb_cache_path(
                                &schist_gallery::thumb_source(&entry.path, entry.edited),
                                entry.mtime,
                            );
                            let meta = schist_gallery::photo_meta(&cache, &entry.path);
                            let text = match xmp::read(&entry.path) {
                                Ok(xmp) => xmp.search_text(),
                                Err(err) => {
                                    log::warn!(
                                        "cannot index metadata for {}: {err:#}",
                                        entry.path.display()
                                    );
                                    String::new()
                                }
                            };
                            (entry.path, meta, text)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |ws, cx| {
                if ws.library.metadata_generation != generation {
                    return;
                }
                ws.library.apply_metadata_rows(rows);
                ws.library.save_index_snapshot();
                if !ws.library.search.text.trim().is_empty() {
                    ws.gallery_search_changed(cx);
                }
                ws.refresh_smart_buckets(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

pub(crate) fn dialog(
    ws: &mut Workspace,
    photos: Vec<PathBuf>,
    values: [String; 6],
    enabled: [bool; 6],
    error: String,
    busy: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(tn("common.n_photos", photos.len() as u64))
        .child(div().text_size(px(11.0)).child(t("metadata.hint")));
    for index in 0..6 {
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().w(px(160.0)).child(crate::ui::checkbox(
                    t(LABELS[index]),
                    enabled[index],
                    move |ws, cx| {
                        ws.commit_focused_field();
                        ws.update_modal(|m| {
                            if let Modal::MetadataEdit { enabled, busy, .. } = m {
                                if !*busy {
                                    enabled[index] = !enabled[index];
                                    if enabled[index] && index == 3 {
                                        enabled[4] = false;
                                    }
                                    if enabled[index] && index == 4 {
                                        enabled[3] = false;
                                    }
                                }
                            }
                        });
                        cx.notify();
                    },
                    cx,
                )))
                .child(super::library_view::bucket_field(
                    IDS[index],
                    values[index].clone(),
                    EXAMPLES[index].into(),
                    ws,
                    cx,
                )),
        );
    }
    body = body.child(
        div()
            .max_h(px(100.0))
            .id("metadata-result")
            .overflow_y_scroll()
            .text_size(px(11.0))
            .child(SharedString::from(error)),
    );
    let actions = div()
        .flex()
        .justify_end()
        .gap_2()
        .child(crate::ui::button(
            t("common.close"),
            false,
            |ws, _w, cx| ws.close_modal(cx),
            cx,
        ))
        .children((!busy && !photos.is_empty()).then(|| {
            crate::ui::button(
                t("common.save"),
                true,
                |ws, _w, cx| ws.save_gallery_metadata(cx),
                cx,
            )
        }))
        .children(busy.then(|| div().child(t("common.saving"))));
    crate::ui::modal_frame(t("metadata.title"), 620.0, body, actions)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_batch_changes_only_checked_fields_and_validates_input() {
        let values = [
            "one; two; one".into(),
            "new caption".into(),
            "copyright".into(),
            "2024-02-29T12:00:00".into(),
            "3600".into(),
            "-33.75, 151.2".into(),
        ];
        let patch = build_patch(&values, &[true, false, false, false, true, true]).unwrap();
        assert_eq!(patch.keywords, Some(vec!["one".into(), "two".into()]));
        assert!(patch.caption.is_none() && patch.copyright.is_none() && patch.taken.is_none());
        assert_eq!(patch.offset_seconds, Some(3600));
        assert_eq!(patch.gps, Some(Some((-33.75, 151.2))));
        assert!(build_patch(&values, &[false, false, false, true, true, false]).is_err());
        let empty: [String; 6] = Default::default();
        let clear = build_patch(&empty, &[true, true, true, true, false, true]).unwrap();
        assert_eq!(clear.gps, Some(None));
        assert_eq!(clear.taken, Some(String::new()));
    }
    #[test]
    fn metadata_focusing_does_not_select_a_field_and_typing_time_modes_is_exclusive() {
        let mut modal = Modal::MetadataEdit {
            id: 1,
            photos: vec![],
            values: Default::default(),
            enabled: [false; 6],
            error: String::new(),
            busy: false,
        };
        assert!(commit_field(&mut modal, IDS[1], String::new()));
        if let Modal::MetadataEdit { enabled, .. } = &modal {
            assert_eq!(*enabled, [false; 6]);
        }
        commit_field(&mut modal, IDS[3], "2024-02-29T12:00:00".into());
        commit_field(&mut modal, IDS[4], "3600".into());
        if let Modal::MetadataEdit { enabled, .. } = &modal {
            assert!(!enabled[3] && enabled[4]);
        }
        commit_field(&mut modal, IDS[3], "2024-03-01T12:00:00".into());
        if let Modal::MetadataEdit { enabled, .. } = &modal {
            assert!(enabled[3] && !enabled[4]);
        }
    }
}
