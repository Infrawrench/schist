//! Geotag a photo selection from GPX tracks: preview on the map, then write
//! positions through the same portable XMP sidecar path as Photo metadata.
use super::*;
use gpui::{prelude::FluentBuilder as _, StatefulInteractiveElement as _};
use schist_gallery::gpx::{self, CaptureTime, MatchOptions, Plan, Tracks};
use schist_gallery::xmp;
use schist_i18n::{t, tf, tn};
use std::sync::atomic::AtomicBool;

pub(super) const GAP_FIELD: &str = "geotag-gap";
pub(super) const OFFSET_FIELD: &str = "geotag-offset";
pub(super) const ZONE_FIELD: &str = "geotag-zone";

pub(super) struct GeoPhoto {
    pub path: PathBuf,
    pub capture: Option<CaptureTime>,
    pub has_gps: bool,
}

/// The session behind `Modal::Geotag`: what the files and photos say, kept
/// off the modal so retyping a field only re-matches in memory.
#[derive(Default)]
pub(super) struct Geotag {
    pub map: super::library_geo::MapState,
    pub files: Vec<PathBuf>,
    pub tracks: Arc<Tracks>,
    pub photos: Arc<Vec<GeoPhoto>>,
    pub cancel: Arc<AtomicBool>,
    /// The inputs the cached plans were computed from.
    plans: Option<(MatchKey, Arc<Vec<Plan>>)>,
}

#[derive(Clone, Copy, PartialEq)]
struct MatchKey {
    options: MatchOptions,
    skip_existing: bool,
}

/// Parse the dialog's three fields; `Err` names the field at fault.
pub(super) fn options(gap: &str, offset: &str, zone: &str) -> Result<MatchOptions, &'static str> {
    let gap = gap.trim();
    let minutes: f64 = if gap.is_empty() {
        5.0
    } else {
        gap.parse().map_err(|_| GAP_FIELD)?
    };
    if !minutes.is_finite() || !(0.0..=1440.0).contains(&minutes) {
        return Err(GAP_FIELD);
    }
    let clock = gpx::parse_duration(offset).ok_or(OFFSET_FIELD)?;
    let zone = zone.trim();
    let local = if zone.is_empty() {
        0
    } else {
        gpx::parse_utc_offset(zone).ok_or(ZONE_FIELD)?
    };
    Ok(MatchOptions {
        max_gap_seconds: (minutes * 60.0).round() as i64,
        clock_offset_seconds: clock,
        local_utc_offset_seconds: local,
    })
}

#[derive(Default, Debug, PartialEq, Eq)]
pub(super) struct Summary {
    pub write: usize,
    pub has_gps: usize,
    pub no_time: usize,
    pub gap: usize,
    pub outside: usize,
}

pub(super) fn summarize(plans: &[Plan]) -> Summary {
    let mut s = Summary::default();
    for plan in plans {
        match plan {
            Plan::Write { .. } => s.write += 1,
            Plan::SkipHasGps => s.has_gps += 1,
            Plan::SkipNoTime => s.no_time += 1,
            Plan::SkipGap => s.gap += 1,
            Plan::SkipOutside => s.outside += 1,
        }
    }
    s
}

pub(super) fn commit_field(modal: &mut Modal, id: &str, buffer: String) {
    if let Modal::Geotag {
        gap,
        offset,
        zone,
        busy: false,
        ..
    } = modal
    {
        match id {
            GAP_FIELD => *gap = buffer,
            OFFSET_FIELD => *offset = buffer,
            ZONE_FIELD => *zone = buffer,
            _ => {}
        }
    }
}

/// Map bounds around every point, padded so the line clears the edge.
fn track_bounds(tracks: &Tracks) -> Option<GeoBounds> {
    let mut points = tracks.segments.iter().flat_map(|s| &s.points);
    let first = points.next()?;
    let mut b = GeoBounds {
        south: first.lat,
        west: first.lon,
        north: first.lat,
        east: first.lon,
    };
    for p in points {
        b.south = b.south.min(p.lat);
        b.north = b.north.max(p.lat);
        b.west = b.west.min(p.lon);
        b.east = b.east.max(p.lon);
    }
    let pad_lat = ((b.north - b.south) * 0.08).max(0.002);
    let pad_lon = ((b.east - b.west) * 0.08).max(0.002);
    Some(GeoBounds {
        south: (b.south - pad_lat).max(-85.0),
        west: (b.west - pad_lon).max(-180.0),
        north: (b.north + pad_lat).min(85.0),
        east: (b.east + pad_lon).min(180.0),
    })
}

impl Workspace {
    /// Gallery ▸ Geotag from GPX Track…: pick one or more GPX files for the
    /// selected still photos.
    pub(crate) fn open_geotag(&mut self, photos: Vec<PathBuf>, cx: &mut Context<Self>) {
        let photos: Vec<_> = photos
            .into_iter()
            .map(|p| schist_gallery::variants::capture(&p))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .filter(|p| !schist_gallery::is_video(p))
            .collect();
        if photos.is_empty() {
            self.status = t("library.geotag.select_photos").into();
            cx.notify();
            return;
        }
        let picker = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: Some(format!("{} (.gpx)", t("library.geotag.add_files")).into()),
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(files))) = picker.await else {
                return;
            };
            this.update(cx, |ws, cx| ws.load_geotag(files, Some(photos), cx))
                .ok();
        })
        .detach();
    }

    /// Add more track files to an open geotag dialog.
    fn add_geotag_files(&mut self, cx: &mut Context<Self>) {
        let picker = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: Some(format!("{} (.gpx)", t("library.geotag.add_files")).into()),
            },
            cx,
        );
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(files))) = picker.await else {
                return;
            };
            this.update(cx, |ws, cx| {
                let mut all = ws.library.geotag.files.clone();
                for file in files {
                    if !all.contains(&file) {
                        all.push(file);
                    }
                }
                ws.load_geotag(all, None, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Read the tracks (and, for a new session, every photo's capture time
    /// and location) in the background, then show the preview.
    fn load_geotag(
        &mut self,
        files: Vec<PathBuf>,
        photos: Option<Vec<PathBuf>>,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.library.geotag;
        state.cancel.store(true, Ordering::Relaxed);
        state.cancel = Arc::default();
        let cancel = state.cancel.clone();
        let fresh = photos.is_some();
        self.library.metadata_edit_generation += 1;
        let id = self.library.metadata_edit_generation;
        let previous = match &self.modal {
            Some(Modal::Geotag {
                gap,
                offset,
                zone,
                skip_existing,
                ..
            }) if !fresh => Some((gap.clone(), offset.clone(), zone.clone(), *skip_existing)),
            _ => None,
        };
        let (gap, offset, zone, skip_existing) =
            previous.unwrap_or_else(|| ("5".into(), "0".into(), String::new(), true));
        self.open_modal(
            Modal::Geotag {
                id,
                gap,
                offset,
                zone,
                skip_existing,
                loading: true,
                error: String::new(),
                busy: false,
            },
            cx,
        );
        let existing = self.library.geotag.photos.clone();
        cx.spawn(async move |this, cx| {
            let job_files = files.clone();
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let tracks = gpx::read_files(&job_files)?;
                    let photos = match photos {
                        Some(paths) => {
                            let mut out = Vec::with_capacity(paths.len());
                            for path in paths {
                                if cancel.load(Ordering::Relaxed) {
                                    anyhow::bail!("cancelled");
                                }
                                let capture = gpx::capture_time(&path);
                                let has_gps =
                                    schist_gallery::photo_meta(&None, &path).gps.is_some();
                                out.push(GeoPhoto {
                                    path,
                                    capture,
                                    has_gps,
                                });
                            }
                            Arc::new(out)
                        }
                        None => existing,
                    };
                    anyhow::Ok((tracks, photos))
                })
                .await;
            this.update(cx, |ws, cx| {
                let current =
                    matches!(&ws.modal, Some(Modal::Geotag { id: open, .. }) if *open == id);
                if !current {
                    return;
                }
                match loaded {
                    Ok((tracks, photos)) => {
                        let state = &mut ws.library.geotag;
                        state.map.tracks = tracks
                            .segments
                            .iter()
                            .map(|s| s.points.iter().map(|p| (p.lat, p.lon)).collect())
                            .collect();
                        if let Some(bounds) = track_bounds(&tracks) {
                            state.map.frame(bounds);
                        }
                        // The computer's zone on the photos' own date is the
                        // likeliest one for a camera set to local time.
                        let zone = photos
                            .iter()
                            .find_map(|p| match p.capture {
                                Some(CaptureTime::Local(t)) => gpx::local_offset_at(t),
                                _ => None,
                            })
                            .map(gpx::format_utc_offset)
                            .unwrap_or_else(|| "+00:00".into());
                        let empty = tracks.timed_points() == 0;
                        state.files = files;
                        state.tracks = Arc::new(tracks);
                        state.photos = photos;
                        state.plans = None;
                        ws.update_modal(|m| {
                            if let Modal::Geotag {
                                loading,
                                zone: field,
                                error,
                                ..
                            } = m
                            {
                                *loading = false;
                                if field.is_empty() {
                                    *field = zone.clone();
                                }
                                *error = if empty {
                                    t("library.geotag.no_points").into()
                                } else {
                                    String::new()
                                };
                            }
                        });
                    }
                    Err(error) => {
                        log::warn!("geotag: {error:#}");
                        ws.update_modal(|m| {
                            if let Modal::Geotag {
                                loading, error: e, ..
                            } = m
                            {
                                *loading = false;
                                *e = tf!("library.geotag.read_failed", error = error);
                            }
                        });
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Current plans for the dialog's inputs, recomputed only when they change.
    fn geotag_plans(&mut self, key: MatchKey) -> Arc<Vec<Plan>> {
        let state = &mut self.library.geotag;
        if let Some((cached, plans)) = &state.plans {
            if *cached == key {
                return plans.clone();
            }
        }
        let plans: Arc<Vec<Plan>> = Arc::new(
            state
                .photos
                .iter()
                .map(|p| {
                    gpx::plan(
                        &state.tracks,
                        p.capture,
                        p.has_gps,
                        key.skip_existing,
                        &key.options,
                    )
                })
                .collect(),
        );
        state.map.markers = plans
            .iter()
            .filter_map(|p| match p {
                Plan::Write { lat, lon, .. } => Some((*lat, *lon)),
                _ => None,
            })
            .collect();
        state.plans = Some((key, plans.clone()));
        plans
    }

    fn apply_geotag(&mut self, cx: &mut Context<Self>) {
        self.commit_focused_field();
        let Some(Modal::Geotag {
            id,
            gap,
            offset,
            zone,
            skip_existing,
            loading: false,
            busy: false,
            ..
        }) = self.modal.clone()
        else {
            return;
        };
        let options = match options(&gap, &offset, &zone) {
            Ok(options) => options,
            Err(_) => {
                self.update_modal(|m| {
                    if let Modal::Geotag { error, .. } = m {
                        *error = t("library.geotag.invalid").into();
                    }
                });
                cx.notify();
                return;
            }
        };
        let plans = self.geotag_plans(MatchKey {
            options,
            skip_existing,
        });
        let writes: Vec<(PathBuf, f64, f64, Option<f64>)> = self
            .library
            .geotag
            .photos
            .iter()
            .zip(plans.iter())
            .filter_map(|(photo, plan)| match plan {
                Plan::Write { lat, lon, ele } => Some((photo.path.clone(), *lat, *lon, *ele)),
                _ => None,
            })
            .collect();
        if writes.is_empty() {
            return;
        }
        self.update_modal(|m| {
            if let Modal::Geotag { busy, error, .. } = m {
                *busy = true;
                error.clear();
            }
        });
        self.status = t("common.saving").into();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcomes = cx
                .background_executor()
                .spawn(async move {
                    writes
                        .into_iter()
                        .map(|(path, lat, lon, ele)| {
                            // Each write keeps the previous packet under
                            // .schist/metadata, like any metadata edit.
                            let patch = xmp::Patch {
                                gps: Some(Some((lat, lon))),
                                altitude: ele.map(Some),
                                ..Default::default()
                            };
                            let result = xmp::write(&path, &patch);
                            (path, result)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |ws, cx| {
                let total = outcomes.len();
                let mut failed: Vec<PathBuf> = Vec::new();
                let mut written = std::collections::HashSet::new();
                for (path, result) in outcomes {
                    match result {
                        Ok(_) => {
                            written.insert(path);
                        }
                        Err(err) => {
                            log::error!("geotag write failed for {}: {err:#}", path.display());
                            failed.push(path);
                        }
                    }
                }
                let count = total - failed.len();
                ws.status = if failed.is_empty() {
                    tn("library.batch.done", count as u64)
                } else {
                    tf!("library.batch.done_partial", n = count, total = total)
                }
                .into();
                // Written photos now have a location; re-read them so a
                // second Apply skips (or overwrites) exactly as shown.
                let state = &mut ws.library.geotag;
                if let Some(photos) = Arc::get_mut(&mut state.photos) {
                    for photo in photos.iter_mut() {
                        if written.contains(&photo.path) {
                            photo.has_gps = true;
                        }
                    }
                }
                state.plans = None;
                if failed.is_empty() {
                    if matches!(&ws.modal, Some(Modal::Geotag { id: open, .. }) if *open == id) {
                        ws.close_modal(cx);
                    }
                } else {
                    ws.update_modal(|m| {
                        if let Modal::Geotag {
                            id: open,
                            busy,
                            error,
                            ..
                        } = m
                        {
                            if *open == id {
                                *busy = false;
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
                    });
                }
                ws.refresh_gallery_metadata(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dialog(
    ws: &mut Workspace,
    gap: String,
    offset: String,
    zone: String,
    skip_existing: bool,
    loading: bool,
    error: String,
    busy: bool,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let dim = gpui::rgb(crate::ui::palette().text_dim);
    let parsed = options(&gap, &offset, &zone);
    let summary = match (&parsed, loading) {
        (Ok(options), false) => {
            let plans = ws.geotag_plans(MatchKey {
                options: *options,
                skip_existing,
            });
            Some(summarize(&plans))
        }
        _ => None,
    };
    let state = &ws.library.geotag;
    let names = state
        .files
        .iter()
        .filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect::<Vec<_>>()
        .join(", ");
    let row = |label: &'static str, id: &'static str, value: String, example: &str| {
        (label, id, value, example.to_string())
    };
    let rows = [
        row("library.geotag.max_gap", GAP_FIELD, gap, "5"),
        row("library.geotag.clock_offset", OFFSET_FIELD, offset, "-1:30"),
        row("library.geotag.time_zone", ZONE_FIELD, zone, "+02:00"),
    ];
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(11.0))
                        .child(format!(
                            "{} · {} · {}",
                            tn("common.n_photos", state.photos.len() as u64),
                            tn(
                                "library.geotag.n_track_points",
                                state.tracks.timed_points() as u64
                            ),
                            names
                        )),
                )
                .child(crate::ui::button(
                    t("library.geotag.add_files"),
                    false,
                    |ws, _w, cx| ws.add_geotag_files(cx),
                    cx,
                )),
        )
        .child(super::library_view::map_element(
            ws,
            super::library_geo::MapSlot::Geotag,
            300.0,
            cx,
        ));
    for (label, id, value, example) in rows {
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().w(px(220.0)).text_size(px(12.0)).child(t(label)))
                .child(super::gallery_metadata::field(id, value, example, ws, cx).w(px(160.0))),
        );
    }
    body = body
        .child(
            div()
                .text_size(px(11.0))
                .text_color(dim)
                .child(t("library.geotag.zone_help")),
        )
        .child(crate::ui::checkbox(
            t("library.geotag.skip_existing"),
            skip_existing,
            |ws, cx| {
                ws.commit_focused_field();
                ws.update_modal(|m| {
                    if let Modal::Geotag {
                        skip_existing,
                        busy: false,
                        ..
                    } = m
                    {
                        *skip_existing = !*skip_existing;
                    }
                });
                cx.notify();
            },
            cx,
        ));
    let mut lines: Vec<String> = Vec::new();
    if loading {
        lines.push(t("common.loading").into());
    } else if parsed.is_err() {
        lines.push(t("library.geotag.invalid").into());
    } else if let Some(s) = &summary {
        lines.push(tn("library.geotag.n_will_tag", s.write as u64));
        for (n, key) in [
            (s.has_gps, "library.geotag.n_has_gps"),
            (s.no_time, "library.geotag.n_no_time"),
            (s.gap, "library.geotag.n_gap"),
            (s.outside, "library.geotag.n_outside"),
        ] {
            if n > 0 {
                lines.push(tn(key, n as u64));
            }
        }
    }
    if !error.is_empty() {
        lines.push(error);
    }
    body = body.child(
        div()
            .id("geotag-summary")
            .max_h(px(100.0))
            .overflow_y_scroll()
            .text_size(px(11.0))
            .children(lines.into_iter().map(|l| div().child(l))),
    );
    let writable = summary.as_ref().is_some_and(|s| s.write > 0);
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
        .when(writable && !busy, |el| {
            el.child(crate::ui::button(
                t("library.geotag.apply"),
                true,
                |ws, _w, cx| ws.apply_geotag(cx),
                cx,
            ))
        })
        .when(busy, |el| el.child(div().child(t("common.saving"))));
    crate::ui::modal_frame(t("library.geotag.title"), 680.0, body, actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_fields_parse_minutes_corrections_and_zones() {
        let o = options("2.5", "-1:30", "+02:00").unwrap();
        assert_eq!(o.max_gap_seconds, 150);
        assert_eq!(o.clock_offset_seconds, -90);
        assert_eq!(o.local_utc_offset_seconds, 7200);
        let defaults = options("", "", "").unwrap();
        assert_eq!(defaults.max_gap_seconds, 300);
        assert_eq!(options("x", "0", "+00:00"), Err(GAP_FIELD));
        assert_eq!(options("-1", "0", "+00:00"), Err(GAP_FIELD));
        assert_eq!(options("5", "1:99", "+00:00"), Err(OFFSET_FIELD));
        assert_eq!(options("5", "0", "Mars"), Err(ZONE_FIELD));
    }

    #[test]
    fn summary_counts_each_outcome() {
        let plans = [
            Plan::Write {
                lat: 1.0,
                lon: 2.0,
                ele: None,
            },
            Plan::SkipHasGps,
            Plan::SkipNoTime,
            Plan::SkipGap,
            Plan::SkipOutside,
            Plan::SkipOutside,
        ];
        assert_eq!(
            summarize(&plans),
            Summary {
                write: 1,
                has_gps: 1,
                no_time: 1,
                gap: 1,
                outside: 2
            }
        );
    }
}
