//! Cloud-backed photo decisions, metadata, review and version history.
use super::gallery_chrome as chrome;
use super::*;
use anyhow::{ensure, Result};
use schist_cloud::gallery::{Target, Version};
use schist_cloud::{
    self as remote,
    protocol::{map, parse, value},
    Asset, Value,
};
use schist_gallery::{
    culling::{ColourLabel, CompareCamera, CullEdit, CullFilter, CullFlag, PhotoCulling},
    similar,
};
use schist_gallery_ui::comparison::{
    self as compare_ui, CompareAction, ComparisonActions, ComparisonPane, ComparisonToolbar,
};
use schist_gallery_ui::culling::{
    self as controls_ui, CullingActions, CullingControls, CullingPopover,
};
use schist_i18n::{t, tf};
use schist_ui::Button;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum View {
    Compare,
    Review,
    Versions,
}
#[derive(Default)]
pub(super) struct State {
    pub view: Option<View>,
    serial: u64,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    busy: bool,
    progress: Option<(usize, usize)>,
    photos: Vec<Asset>,
    groups: Vec<Vec<usize>>,
    group: usize,
    candidate: usize,
    versions: Vec<Version>,
    version: usize,
    images: [Option<(Arc<RenderImage>, [f32; 2])>; 2],
    camera: CompareCamera,
    active: usize,
    bounds: [Bounds<Pixels>; 2],
    drag: Option<Point<Pixels>>,
    error: Option<String>,
}
impl State {
    pub(super) fn close(&mut self) {
        self.serial += 1;
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.cancel = Arc::default();
        self.view = None;
        self.images = [None, None];
        self.busy = false;
        self.progress = None;
        self.error = None;
    }
    pub(super) fn reconcile(&mut self, assets: &[Asset]) {
        for old in &mut self.photos {
            if let Some(asset) = assets.iter().find(|a| {
                a.id == old.id
                    && a.revision == old.revision
                    && a.metadata.metadata_revision >= old.metadata.metadata_revision
            }) {
                *old = asset.clone();
            }
        }
    }
    fn pair(&self) -> Vec<Asset> {
        if self.view == Some(View::Review) {
            self.groups
                .get(self.group)
                .map(|group| {
                    [0, self.candidate.min(group.len() - 1)]
                        .iter()
                        .map(|&i| self.photos[group[i]].clone())
                        .collect()
                })
                .unwrap_or_default()
        } else {
            self.photos.iter().take(2).cloned().collect()
        }
    }
}
pub(super) enum Event {
    Progress {
        serial: u64,
        done: usize,
        total: usize,
    },
    Reply {
        serial: u64,
        method: &'static str,
        result: std::result::Result<Value, String>,
    },
    Review {
        serial: u64,
        result: std::result::Result<(Vec<Asset>, Vec<Vec<usize>>), String>,
    },
    Preview {
        serial: u64,
        slot: usize,
        result: std::result::Result<(u32, u32, Vec<u8>), String>,
    },
}
fn feature(ws: &Workspace, name: &str) -> bool {
    ws.cloud.connected
        && ws
            .cloud
            .capabilities
            .as_ref()
            .is_some_and(|c| c.supports_gallery(name))
}
fn targets(photos: &[Asset]) -> Result<Value> {
    Ok(value(photos.iter().map(Target::from).collect::<Vec<_>>()))
}
impl Workspace {
    fn cloud_gallery_photos(&self) -> Vec<Asset> {
        if self.cloud.gallery.view.is_some() {
            let pair = self.cloud.gallery.pair();
            pair.get(self.cloud.gallery.active)
                .cloned()
                .into_iter()
                .collect()
        } else {
            self.cloud
                .assets
                .iter()
                .filter(|a| self.cloud.selected.contains(&a.id))
                .cloned()
                .collect()
        }
    }
    fn cloud_gallery_request(&mut self, method: &'static str, params: Value) {
        let Some(client) = &self.cloud.client else {
            return;
        };
        let (handle, sender, epoch, serial) = (
            client.handle.clone(),
            self.cloud.sender.clone(),
            self.cloud.epoch,
            self.cloud.gallery.serial,
        );
        self.cloud.gallery.busy = true;
        self.cloud.gallery.error = None;
        remote::runtime::spawn(async move {
            let result = handle
                .request_async(method, params)
                .await
                .map_err(|e| e.to_string());
            let _ = sender.send(super::cloud::Job::Gallery {
                epoch,
                event: Event::Reply {
                    serial,
                    method,
                    result,
                },
            });
        });
    }
    fn cloud_gallery_mutation(&mut self, method: &'static str, fields: Vec<(&'static str, Value)>) {
        let mut fields = fields;
        fields.push(("mutation_id", remote::Uuid::new_v4().to_string().into()));
        self.cloud_gallery_request(method, map(fields));
    }
    pub(super) fn cloud_cull(
        &mut self,
        field: &'static str,
        decision: Value,
        cx: &mut Context<Self>,
    ) {
        if !feature(self, "culling") || self.cloud.gallery.busy {
            return;
        }
        let photos = self.cloud_gallery_photos();
        if photos.is_empty() {
            return;
        }
        if let Ok(assets) = targets(&photos) {
            self.cloud_gallery_mutation(
                "assets.culling.update",
                vec![("assets", assets), ("patch", map([(field, decision)]))],
            );
        }
        cx.notify();
    }
    pub(super) fn cloud_gallery_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let serial = match &event {
            Event::Progress { serial, .. }
            | Event::Reply { serial, .. }
            | Event::Review { serial, .. }
            | Event::Preview { serial, .. } => *serial,
        };
        if serial != self.cloud.gallery.serial {
            return;
        }
        let result = (|| -> Result<()> {
            match event {
                Event::Progress { done, total, .. } => {
                    self.cloud.gallery.progress = Some((done, total))
                }
                Event::Reply { method, result, .. } => {
                    self.cloud.gallery.busy = false;
                    let result = result.map_err(anyhow::Error::msg)?;
                    match method {
                        "asset.versions" => {
                            #[derive(serde::Deserialize)]
                            struct Reply {
                                versions: Vec<Version>,
                            }
                            self.cloud.gallery.versions = parse::<Reply>(result)?.versions;
                            self.cloud.gallery.version = 0;
                            self.cloud_gallery_previews();
                        }
                        "asset.version.create" => {
                            if let Some(asset) = self.cloud.gallery.photos.first() {
                                self.cloud_gallery_request(
                                    "asset.versions",
                                    map([("id", asset.id.clone().into())]),
                                );
                            }
                        }
                        "asset.version.restore" => {
                            self.cloud.gallery.close();
                            self.cloud.message = t("common.done").into();
                            self.cloud_watch_assets(true);
                        }
                        "asset.metadata" => {
                            #[derive(serde::Deserialize)]
                            struct Reply {
                                asset: Asset,
                                xmp_url: String,
                            }
                            let reply: Reply = parse(result)?;
                            self.cloud_gallery_download(
                                reply.xmp_url,
                                format!("{}.xmp", reply.asset.name),
                            );
                        }
                        _ => {
                            #[derive(serde::Deserialize)]
                            struct Reply {
                                assets: Vec<Asset>,
                            }
                            let reply: Reply = parse(result)?;
                            for asset in reply.assets {
                                for old in self
                                    .cloud
                                    .assets
                                    .iter_mut()
                                    .chain(self.cloud.gallery.photos.iter_mut())
                                    .filter(|a| a.id == asset.id)
                                {
                                    *old = asset.clone();
                                }
                            }
                            self.cloud.message = t("common.done").into();
                        }
                    }
                }
                Event::Review { result, .. } => {
                    self.cloud.gallery.busy = false;
                    self.cloud.gallery.progress = None;
                    let (photos, groups) = result.map_err(anyhow::Error::msg)?;
                    self.cloud.gallery.photos = photos;
                    self.cloud.gallery.groups = groups;
                    self.cloud.gallery.group = 0;
                    self.cloud.gallery.candidate = 1;
                    self.cloud_gallery_previews();
                }
                Event::Preview { slot, result, .. } => {
                    let (w, h, rgba) = result.map_err(anyhow::Error::msg)?;
                    self.cloud.gallery.images[slot] =
                        super::cloud::rgba_to_render_image(w, h, rgba)
                            .map(|image| (image, [w as f32, h as f32]));
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.cloud.gallery.error = Some(error.to_string());
            self.cloud_error(error.to_string());
        }
        cx.notify();
    }
    fn cloud_gallery_download(&self, url: String, name: String) {
        let (sender, epoch) = (self.cloud.sender.clone(), self.cloud.epoch);
        remote::runtime::spawn(async move {
            let job = match remote::auth::download_limited_async(&url, 512 * 1024 * 1024).await {
                Ok(bytes) => super::cloud::Job::Downloaded {
                    epoch,
                    name,
                    download: remote::transfer::DownloadedAsset {
                        bytes,
                        revision: 0,
                        content_type: None,
                        content_disposition: None,
                        format: None,
                    },
                },
                Err(error) => super::cloud::Job::Error {
                    epoch,
                    error: error.to_string(),
                },
            };
            let _ = sender.send(job);
        });
    }
    pub(super) fn cloud_compare(&mut self, cx: &mut Context<Self>) {
        let photos = self.cloud_gallery_photos();
        if photos.len() != 2 || photos.iter().any(|a| a.mime_type.starts_with("video/")) {
            return;
        }
        self.cloud.gallery.close();
        self.cloud.gallery.view = Some(View::Compare);
        self.cloud.gallery.photos = photos;
        self.cloud.gallery.active = 1;
        self.cloud.gallery.camera = CompareCamera::default();
        self.cloud_gallery_previews();
        cx.notify();
    }
    pub(super) fn cloud_versions(&mut self, cx: &mut Context<Self>) {
        let photos = self.cloud_gallery_photos();
        if photos.len() != 1 {
            return;
        }
        let id = photos[0].id.clone();
        self.cloud.gallery.close();
        self.cloud.gallery.view = Some(View::Versions);
        self.cloud.gallery.active = 0;
        self.cloud.gallery.photos = photos;
        self.cloud.gallery.versions.clear();
        self.cloud.gallery.camera = CompareCamera::default();
        self.cloud_gallery_request("asset.versions", map([("id", id.into())]));
        cx.notify();
    }
    fn cloud_gallery_previews(&mut self) {
        self.cloud.gallery.serial += 1;
        self.cloud.gallery.images = [None, None];
        let Some(client) = &self.cloud.client else {
            return;
        };
        let pair = self.cloud.gallery.pair();
        for slot in 0..2 {
            let version_url = (self.cloud.gallery.view == Some(View::Versions) && slot == 1)
                .then(|| {
                    self.cloud
                        .gallery
                        .versions
                        .get(self.cloud.gallery.version)
                        .map(|v| v.thumbnail_url.clone())
                })
                .flatten();
            let asset = pair.get(slot).cloned();
            if version_url.is_none() && asset.is_none() {
                continue;
            }
            let (handle, sender, epoch, serial) = (
                client.handle.clone(),
                self.cloud.sender.clone(),
                self.cloud.epoch,
                self.cloud.gallery.serial,
            );
            remote::runtime::spawn(async move {
                let result = async {
                    let url = if let Some(url) = version_url {
                        url
                    } else {
                        #[derive(serde::Deserialize)]
                        struct Reply {
                            url: String,
                        }
                        parse::<Reply>(
                            handle
                                .request_async(
                                    "asset.preview",
                                    map([("id", asset.unwrap().id.into())]),
                                )
                                .await?,
                        )?
                        .url
                    };
                    let bytes =
                        remote::auth::download_limited_async(&url, 64 * 1024 * 1024).await?;
                    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                        .with_guessed_format()?;
                    let mut limits = image::Limits::default();
                    limits.max_alloc = Some(128 * 1024 * 1024);
                    limits.max_image_width = Some(8192);
                    limits.max_image_height = Some(8192);
                    reader.limits(limits);
                    let image = reader.decode()?.into_rgba8();
                    Ok::<_, anyhow::Error>((image.width(), image.height(), image.into_raw()))
                }
                .await
                .map_err(|e| e.to_string());
                let _ = sender.send(super::cloud::Job::Gallery {
                    epoch,
                    event: Event::Preview {
                        serial,
                        slot,
                        result,
                    },
                });
            });
        }
    }
    pub(super) fn cloud_review(&mut self, burst: bool, cx: &mut Context<Self>) {
        let Some(client) = &self.cloud.client else {
            return;
        };
        let handle = client.handle.clone();
        self.cloud.gallery.close();
        self.cloud.gallery.view = Some(View::Review);
        self.cloud.gallery.photos.clear();
        self.cloud.gallery.groups.clear();
        self.cloud.gallery.busy = true;
        let (sender, epoch, serial, mut query) = (
            self.cloud.sender.clone(),
            self.cloud.epoch,
            self.cloud.gallery.serial,
            self.cloud.query.clone(),
        );
        query.offset = 0;
        query.limit = 500;
        let cancel = self.cloud.gallery.cancel.clone();
        remote::runtime::spawn(async move {
            let result = async {
                let mut assets = Vec::new();
                loop {
                    ensure!(!cancel.load(std::sync::atomic::Ordering::Relaxed), t("common.cancel"));
                    let page: remote::Snapshot = parse(handle.request_async("assets.query", value(&query)).await?)?;
                    let rows: Vec<Asset> = parse(Value::Array(page.items))?;
                    let count = rows.len();
                    assets.extend(rows.into_iter().filter(|a| !a.mime_type.starts_with("video/")));
                    query.offset += count as u64;
                    if count == 0 || query.offset >= page.total || query.offset >= 10_000 { break; }
                }
                let mut photos = Vec::new();
                let mut displayed = Vec::new();
                for (index, batch) in assets.chunks(1).enumerate() {
                    ensure!(!cancel.load(std::sync::atomic::Ordering::Relaxed), t("common.cancel"));
                    let signatures = if burst { Vec::new() } else {
                        let signatures = handle.review_photos(batch).await?;
                        // Leave room under the shared socket's request budget for
                        // subscriptions, workflow sync and heartbeat frames.
                        remote::runtime::sleep(Duration::from_millis(350)).await;
                        signatures
                    };
                    let _ = sender.send(super::cloud::Job::Gallery { epoch, event: Event::Progress {serial, done: index + 1, total: assets.len()} });
                    for asset in batch {
                        let signature = if burst { None } else {
                            let Some(found) = signatures.iter().find(|p| p.asset.id == asset.id) else { continue; };
                            let hash = u64::from_str_radix(&found.signature.hash, 16)?;
                            Some(serde_json::from_value::<similar::Signature>(serde_json::json!({ "hash": hash, "rgb": found.signature.rgb, "aspect": found.signature.aspect }))?)
                        };
                        photos.push(similar::Photo {
                            path: PathBuf::from(asset.folder_id.as_deref().unwrap_or("root")).join(&asset.id),
                            stamp: similar::Stamp { bytes: asset.size, seconds: asset.revision, nanos: 0 },
                            signature, captured: asset.captured_at.and_then(|t| i64::try_from(t).ok()),
                        });
                        displayed.push(asset.clone());
                    }
                }
                let groups = similar::groups(&photos, if burst { similar::Mode::Burst } else { similar::Mode::Visual }, 6, &cancel);
                Ok::<_, anyhow::Error>((displayed, groups.into_iter().map(|g| g.photos).collect()))
            }.await.map_err(|e| e.to_string());
            let _ = sender.send(super::cloud::Job::Gallery {
                epoch,
                event: Event::Review { serial, result },
            });
        });
        cx.notify();
    }
    fn cloud_review_choice(&mut self, choice: Option<&'static str>, cx: &mut Context<Self>) {
        if self.cloud.gallery.busy {
            return;
        }
        let Some(group) = self.cloud.gallery.groups.get(self.cloud.gallery.group) else {
            return;
        };
        let photos: Vec<_> = group
            .iter()
            .map(|&i| self.cloud.gallery.photos[i].clone())
            .collect();
        let pair = self.cloud.gallery.pair();
        let Some(asset) = pair.get(self.cloud.gallery.active) else {
            return;
        };
        if let Ok(group) = targets(&photos) {
            self.cloud_gallery_mutation(
                "assets.review.update",
                vec![
                    ("group", group),
                    ("id", asset.id.clone().into()),
                    ("choice", choice.map(Value::from).unwrap_or(Value::Nil)),
                ],
            );
        }
        cx.notify();
    }
    pub(super) fn cloud_gallery_key(
        &mut self,
        ev: &gpui::KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.cloud.search.active
            || self.focused_field.is_some()
            || self.ai.input.active
            || self.ai.model_menu
            || self.spotlight.open
        {
            return false;
        }
        let m = ev.keystroke.modifiers;
        if m.control || m.platform || m.alt || m.function {
            return false;
        }
        let key = ev.keystroke.key.as_str();
        if self.cloud.gallery.view.is_some() {
            match key {
                "escape" => self.cloud.gallery.close(),
                "left" => self.cloud.gallery.active = 0,
                "right" => self.cloud.gallery.active = 1,
                "tab" => self.cloud.gallery.active = 1 - self.cloud.gallery.active,
                "+" | "=" => self.cloud.gallery.camera.zoom_by(1.25),
                "-" => self.cloud.gallery.camera.zoom_by(0.8),
                "f" => self.cloud.gallery.camera = CompareCamera::default(),
                _ => return self.cloud_cull_key(key, cx),
            }
            cx.notify();
            return true;
        }
        if m.shift {
            return false;
        }
        if key == "c" && feature(self, "culling") {
            self.cloud_compare(cx);
            return true;
        }
        self.cloud_cull_key(key, cx)
    }
    fn cloud_cull_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if !feature(self, "culling") {
            return false;
        }
        let Some(edit) = controls_ui::shortcut(key) else {
            return false;
        };
        self.cloud_cull_edit(edit, cx);
        true
    }
}

const FIELD_IDS: [&str; 6] = [
    "cloud-meta-keywords",
    "cloud-meta-caption",
    "cloud-meta-copyright",
    "cloud-meta-taken",
    "cloud-meta-offset",
    "cloud-meta-gps",
];
const CHECK_IDS: [&str; 6] = [
    "cloud-check-keywords",
    "cloud-check-caption",
    "cloud-check-copyright",
    "cloud-check-taken",
    "cloud-check-offset",
    "cloud-check-gps",
];
impl Workspace {
    pub(super) fn cloud_metadata_editor(&mut self, cx: &mut Context<Self>) {
        let photos = self.cloud_gallery_photos();
        if photos.is_empty() {
            return;
        }
        let mut values: [String; 6] = Default::default();
        if photos.len() == 1 {
            let a = &photos[0];
            values = [
                a.tags.join("; "),
                a.metadata.caption.clone(),
                a.metadata.copyright.clone(),
                remote::gallery::format_time(a.captured_at),
                String::new(),
                a.location
                    .as_ref()
                    .map(|p| format!("{}, {}", p.latitude, p.longitude))
                    .unwrap_or_default(),
            ];
        }
        let Ok(encoded) =
            serde_json::to_string(&photos.iter().map(Target::from).collect::<Vec<_>>())
        else {
            return;
        };
        let mut fields = vec![("cloud-meta-targets", String::new(), encoded)];
        for i in 0..6 {
            fields.push((
                CHECK_IDS[i],
                t(super::gallery_metadata::LABELS[i]).into(),
                String::new(),
            ));
            fields.push((
                FIELD_IDS[i],
                t(super::gallery_metadata::LABELS[i]).into(),
                values[i].clone(),
            ));
        }
        self.open_modal(
            Modal::Cloud {
                kind: "metadata",
                fields,
            },
            cx,
        );
    }
}
fn metadata_enabled(fields: &[(&str, String, String)]) -> [bool; 6] {
    std::array::from_fn(|i| {
        fields
            .iter()
            .any(|(key, _, value)| *key == CHECK_IDS[i] && value == "1")
    })
}

fn set_metadata_enabled(fields: &mut [(&str, String, String)], enabled: [bool; 6]) {
    for (key, _, value) in fields {
        if let Some(index) = CHECK_IDS.iter().position(|id| id == key) {
            *value = if enabled[index] { "1" } else { "" }.into();
        }
    }
}

pub(super) fn commit_metadata_field(
    fields: &mut [(&str, String, String)],
    id: &str,
    buffer: &str,
) -> bool {
    let Some(index) = FIELD_IDS.iter().position(|key| *key == id) else {
        return false;
    };
    let Some((_, _, value)) = fields.iter_mut().find(|(key, _, _)| *key == id) else {
        return false;
    };
    if value != buffer {
        *value = buffer.into();
        let mut enabled = metadata_enabled(fields);
        super::gallery_metadata::enable_field(&mut enabled, index, true);
        set_metadata_enabled(fields, enabled);
    }
    true
}

pub(super) fn metadata_dialog(
    ws: &mut Workspace,
    fields: Vec<(&'static str, String, String)>,
    cx: &mut Context<Workspace>,
) -> gpui::AnyElement {
    let get = |id| {
        fields
            .iter()
            .find(|(key, _, _)| *key == id)
            .map(|(_, _, value)| value.clone())
            .unwrap_or_default()
    };
    let count = serde_json::from_str::<Vec<Target>>(&get("cloud-meta-targets"))
        .map_or(0, |targets| targets.len());
    let busy = ws.cloud.gallery.busy;
    super::gallery_metadata::dialog(
        ws,
        FIELD_IDS,
        super::gallery_metadata::MetadataForm {
            count,
            values: std::array::from_fn(|i| get(FIELD_IDS[i])),
            enabled: metadata_enabled(&fields),
            error: get("cloud-meta-error"),
            busy,
        },
        super::gallery_metadata::MetadataActions {
            toggle: |ws, index, cx| {
                if !ws.cloud.gallery.busy {
                    ws.update_modal(|m| {
                        if let Modal::Cloud {
                            kind: "metadata",
                            fields,
                        } = m
                        {
                            let mut enabled = metadata_enabled(fields);
                            let checked = !enabled[index];
                            super::gallery_metadata::enable_field(&mut enabled, index, checked);
                            set_metadata_enabled(fields, enabled);
                        }
                    });
                }
                cx.notify();
            },
            save: |ws, cx| {
                ws.commit_focused_field();
                let Some(Modal::Cloud {
                    kind: "metadata",
                    fields,
                }) = ws.modal.clone()
                else {
                    return;
                };
                match ws.cloud_submit("metadata", fields, cx) {
                    Ok(()) => ws.close_modal(cx),
                    Err(error) => {
                        let error = error.to_string();
                        ws.status = error.clone().into();
                        ws.cloud.message = error.clone();
                        ws.update_modal(|m| {
                            if let Modal::Cloud { fields, .. } = m {
                                fields.retain(|(key, _, _)| *key != "cloud-meta-error");
                                fields.push(("cloud-meta-error", String::new(), error.clone()));
                            }
                        });
                        cx.notify();
                    }
                }
            },
        },
        cx,
    )
}

pub(super) fn submit(
    ws: &mut Workspace,
    kind: &str,
    fields: &[(&str, String, String)],
    cx: &mut Context<Workspace>,
) -> Result<bool> {
    if kind != "metadata" {
        return Ok(false);
    }
    let get = |key| {
        fields
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, _, v)| v.as_str())
            .unwrap_or("")
    };
    ensure!(!ws.cloud.gallery.busy, t("common.loading"));
    let assets: Vec<Target> = serde_json::from_str(get("cloud-meta-targets"))?;
    let mut patch = Vec::new();
    for i in 0..6 {
        if get(CHECK_IDS[i]) != "1" {
            continue;
        }
        let text = get(FIELD_IDS[i]);
        patch.push(match i {
            0 => (
                "keywords",
                value(
                    text.split(';')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>(),
                ),
            ),
            1 => ("caption", text.into()),
            2 => ("copyright", text.into()),
            3 => {
                ensure!(get(CHECK_IDS[4]) != "1", t("metadata.invalid"));
                (
                    "captured_at",
                    remote::gallery::parse_time(text)?
                        .map(Value::from)
                        .unwrap_or(Value::Nil),
                )
            }
            4 => (
                "offset_seconds",
                Value::from(
                    text.trim()
                        .parse::<i64>()
                        .map_err(|_| anyhow::anyhow!(t("metadata.invalid")))?,
                ),
            ),
            _ => {
                let location = if text.trim().is_empty() {
                    Value::Nil
                } else {
                    let (a, b) = text
                        .split_once(',')
                        .ok_or_else(|| anyhow::anyhow!(t("metadata.invalid")))?;
                    let (lat, lon) = (a.trim().parse::<f64>()?, b.trim().parse::<f64>()?);
                    ensure!(
                        lat.is_finite()
                            && lon.is_finite()
                            && lat.abs() <= 90.0
                            && lon.abs() <= 180.0,
                        t("metadata.invalid")
                    );
                    map([
                        ("latitude", Value::F64(lat)),
                        ("longitude", Value::F64(lon)),
                    ])
                };
                ("location", location)
            }
        });
    }
    ensure!(!patch.is_empty(), t("metadata.invalid"));
    ws.cloud_gallery_mutation(
        "assets.metadata.update",
        vec![("assets", value(&assets)), ("patch", map(patch))],
    );
    cx.notify();
    Ok(true)
}

fn flag(value: &str) -> CullFlag {
    match value {
        "pick" => CullFlag::Pick,
        "reject" => CullFlag::Reject,
        _ => CullFlag::None,
    }
}

fn label(value: &str) -> ColourLabel {
    match value {
        "red" => ColourLabel::Red,
        "yellow" => ColourLabel::Yellow,
        "green" => ColourLabel::Green,
        "blue" => ColourLabel::Blue,
        "magenta" => ColourLabel::Magenta,
        _ => ColourLabel::None,
    }
}

fn culling_value(asset: &Asset) -> PhotoCulling {
    PhotoCulling {
        rating: asset.rating.min(5),
        flag: flag(&asset.metadata.flag),
        label: label(&asset.metadata.label),
    }
}

fn culling_filter(filters: &remote::Filters) -> CullFilter {
    CullFilter {
        minimum_rating: filters.min_rating.unwrap_or(0),
        flag: filters.flag.as_deref().map(flag),
        label: filters.label.as_deref().map(label),
    }
}

fn set_culling_filter(filters: &mut remote::Filters, filter: CullFilter) {
    filters.min_rating = (filter.minimum_rating > 0).then_some(filter.minimum_rating);
    filters.flag = filter.flag.map(|flag| flag_key(flag).into());
    filters.label = filter.label.map(|label| label_key(label).into());
}

fn flag_key(flag: CullFlag) -> &'static str {
    match flag {
        CullFlag::None => "none",
        CullFlag::Pick => "pick",
        CullFlag::Reject => "reject",
    }
}

fn label_key(label: ColourLabel) -> &'static str {
    match label {
        ColourLabel::None => "none",
        ColourLabel::Red => "red",
        ColourLabel::Yellow => "yellow",
        ColourLabel::Green => "green",
        ColourLabel::Blue => "blue",
        ColourLabel::Magenta => "magenta",
    }
}

fn culling_patch(edit: CullEdit) -> (&'static str, Value) {
    match edit {
        CullEdit::Rating(rating) => ("rating", rating.into()),
        CullEdit::Flag(flag) => ("flag", flag_key(flag).into()),
        CullEdit::Label(label) => ("label", label_key(label).into()),
    }
}

impl Workspace {
    fn cloud_cull_edit(&mut self, edit: CullEdit, cx: &mut Context<Self>) {
        let (field, value) = culling_patch(edit);
        self.cloud_cull(field, value, cx);
    }
}

pub(super) fn badge(asset: &Asset) -> Option<gpui::AnyElement> {
    controls_ui::badge(culling_value(asset))
}

const CULLING_POPUP: Popup = Popup::Field("cloud-gallery-culling");

pub(super) fn toolbar(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let mut bar = div().flex().items_center().gap_2();
    if ws.cloud.batch_cancel.is_some() {
        bar = bar.child(
            Button::new("cloud-batch-cancel", t("common.cancel")).on_click(cx.listener(
                |ws, _, _, cx| {
                    if let Some(cancel) = &ws.cloud.batch_cancel {
                        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    cx.notify();
                },
            )),
        );
    }
    if !feature(ws, "culling") {
        return bar.into_any_element();
    }
    let open = ws.open_popup == Some(CULLING_POPUP);
    let filter = culling_filter(&ws.cloud.query.filters);
    let content = open.then(|| {
        let photos = ws.cloud_gallery_photos();
        controls_ui::controls(
            CullingControls {
                values: photos.iter().map(culling_value).collect(),
                busy: ws.cloud.gallery.busy,
                comparing: ws.cloud.gallery.view.is_some(),
                can_compare: photos.len() == 2
                    && !photos.iter().any(|a| a.mime_type.starts_with("video/")),
                filter,
                max_height: (ws.visible_height - 100.0).max(120.0),
            },
            CullingActions {
                edit: Workspace::cloud_cull_edit,
                filter: |ws, action, cx| {
                    let filter = action.apply(culling_filter(&ws.cloud.query.filters));
                    set_culling_filter(&mut ws.cloud.query.filters, filter);
                    ws.cloud_watch_assets(true);
                    cx.notify();
                },
                compare: |ws, cx| {
                    ws.close_popup(cx);
                    ws.cloud_compare(cx);
                },
            },
            cx,
        )
    });
    bar.child(controls_ui::toolbar(
        CullingPopover {
            open,
            filtered: filter != CullFilter::default(),
            compact: ws.gallery_compact,
        },
        content,
        |ws, cx| {
            ws.commit_focused_field();
            ws.cloud.search.active = false;
            ws.gallery_more = None;
            ws.toggle_popup(CULLING_POPUP, cx);
        },
        |ws, cx| ws.close_popup(cx),
        cx,
    ))
    .into_any_element()
}

pub(super) fn menu(
    ws: &mut Workspace,
    rows: &mut Vec<gpui::AnyElement>,
    cx: &mut Context<Workspace>,
) {
    let dismiss: fn(&mut Workspace) = |ws| ws.cloud.context = None;
    let mut row = |key: &str, act: chrome::MenuAction| {
        rows.push(chrome::menu_row(
            if key == "common.export" {
                format!("{} (XMP)", t(key))
            } else {
                t(key).into()
            },
            dismiss,
            act,
            cx,
        ))
    };
    if feature(ws, "metadata") {
        row(
            "metadata.title",
            std::rc::Rc::new(|ws, _, cx| ws.cloud_metadata_editor(cx)),
        );
        if ws.cloud.selected.len() == 1 {
            row(
                "common.export",
                std::rc::Rc::new(|ws, _, cx| {
                    if let Some(a) = ws.cloud_lead_asset() {
                        ws.cloud_gallery_request("asset.metadata", map([("id", a.id.into())]));
                        cx.notify();
                    }
                }),
            );
        }
    }
    if feature(ws, "culling") && ws.cloud.selected.len() == 2 {
        row(
            "culling.compare",
            std::rc::Rc::new(|ws, _, cx| ws.cloud_compare(cx)),
        );
    }
    if feature(ws, "versions") && ws.cloud.selected.len() == 1 {
        row(
            "versions.open",
            std::rc::Rc::new(|ws, _, cx| ws.cloud_versions(cx)),
        );
    }
    row(
        "actions.title",
        std::rc::Rc::new(|ws, _, cx| ws.open_actions(cx)),
    );
    row(
        "export_recipes.title",
        std::rc::Rc::new(|ws, _, cx| ws.cloud_export_recipes(cx)),
    );
    if feature(ws, "review") {
        row(
            "library.similar.visual",
            std::rc::Rc::new(|ws, _, cx| ws.cloud_review(false, cx)),
        );
        row(
            "library.similar.burst",
            std::rc::Rc::new(|ws, _, cx| ws.cloud_review(true, cx)),
        );
    }
}

pub(super) fn view(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let kind = ws.cloud.gallery.view.unwrap();
    let state = &ws.cloud.gallery;
    let header = compare_ui::toolbar(
        ComparisonToolbar {
            title: t(match kind {
                View::Compare => "culling.compare",
                View::Review => "library.similar.title",
                View::Versions => "versions.title",
            }),
            zoom: state.camera.zoom,
            actual_size_enabled: state.images[state.active].is_some(),
        },
        |ws, action, cx| {
            let s = &mut ws.cloud.gallery;
            match action {
                CompareAction::Close => s.close(),
                CompareAction::Fit => s.camera = CompareCamera::default(),
                CompareAction::ActualSize => {
                    if let Some((_, dimensions)) = &s.images[s.active] {
                        let size = s.bounds[s.active].size;
                        s.camera
                            .actual_size(*dimensions, [size.width.into(), size.height.into()]);
                    }
                }
                CompareAction::ZoomOut => s.camera.zoom_by(0.8),
                CompareAction::ZoomIn => s.camera.zoom_by(1.25),
            }
            cx.notify();
        },
        cx,
    );
    let mut bar = div().flex().flex_wrap().gap_2().items_center().p_2();
    if kind == View::Review {
        let state = &ws.cloud.gallery;
        if let Some((done, total)) = state.progress {
            bar = bar.child(format!("{done}/{total}"));
        }
        bar = bar.child(format!(
            "{}/{}",
            (state.group + 1).min(state.groups.len()),
            state.groups.len()
        ));
        for (i, forward) in [false, true].into_iter().enumerate() {
            bar = bar.child(
                Button::new(("cloud-review-group", i), if forward { "→" } else { "←" })
                    .disabled(state.busy || state.groups.is_empty())
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        let s = &mut ws.cloud.gallery;
                        s.group = if forward {
                            (s.group + 1).min(s.groups.len().saturating_sub(1))
                        } else {
                            s.group.saturating_sub(1)
                        };
                        s.candidate = 1;
                        ws.cloud_gallery_previews();
                        cx.notify();
                    })),
            );
        }
        let candidates = state.groups.get(state.group).map_or(0, Vec::len);
        for i in 1..candidates {
            bar = bar.child(
                Button::new(("cloud-review-candidate", i), (i + 1).to_string())
                    .disabled(state.busy)
                    .active(state.candidate == i)
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.cloud.gallery.candidate = i;
                        ws.cloud_gallery_previews();
                        cx.notify();
                    })),
            );
        }
        for (i, (v, k)) in [
            (Some("keep"), "library.similar.keep"),
            (Some("reject"), "library.similar.reject"),
            (None, "common.reset"),
        ]
        .into_iter()
        .enumerate()
        {
            bar = bar.child(
                Button::new(("cloud-review-choice", i), t(k))
                    .disabled(state.busy)
                    .on_click(cx.listener(move |ws, _, _, cx| ws.cloud_review_choice(v, cx))),
            );
        }
    }
    if kind == View::Versions {
        let state = &ws.cloud.gallery;
        bar = bar.child(
            Button::new("cloud-version-save", t("common.save"))
                .disabled(state.busy)
                .on_click(cx.listener(|ws, _, _, cx| {
                    if let Some(asset) = ws.cloud.gallery.photos.first().cloned() {
                        ws.cloud_gallery_mutation(
                            "asset.version.create",
                            vec![("id", asset.id.into()), ("revision", asset.revision.into())],
                        );
                        cx.notify();
                    }
                })),
        );
        for (i, version) in state.versions.iter().enumerate() {
            bar = bar.child(
                Button::new(
                    ("cloud-version", i),
                    remote::gallery::format_time(Some(version.created_at)),
                )
                .disabled(state.busy)
                .active(state.version == i)
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.cloud.gallery.version = i;
                    ws.cloud_gallery_previews();
                    cx.notify();
                })),
            );
        }
        let version = state.versions.get(state.version).cloned();
        let source = state.photos.first().cloned();
        if let (Some(version), Some(source)) = (version, source) {
            let download = version.clone();
            bar = bar.child(
                Button::new("cloud-version-download", t("cloud.gallery.download")).on_click(
                    cx.listener(move |ws, _, _, _| {
                        ws.cloud_gallery_download(
                            download.download_url.clone(),
                            format!("{}.psd", download.name.trim_end_matches(".psd")),
                        )
                    }),
                ),
            );
            bar = bar.child(
                Button::new("cloud-version-restore", t("versions.restore"))
                    .disabled(state.busy)
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.cloud_gallery_mutation(
                            "asset.version.restore",
                            vec![
                                ("id", source.id.clone().into()),
                                ("revision", source.revision.into()),
                                ("version_id", version.id.clone().into()),
                                (
                                    "name",
                                    format!(
                                        "{}.psd",
                                        tf!(
                                            "versions.copy_title",
                                            name = source.name.trim_end_matches(".psd")
                                        )
                                    )
                                    .into(),
                                ),
                            ],
                        );
                        cx.notify();
                    })),
            );
        }
    }
    let pair = ws.cloud.gallery.pair();
    let mut panes = compare_ui::pane_row();
    for slot in 0..2 {
        let state = &ws.cloud.gallery;
        let title = if kind == View::Versions && slot == 1 {
            state
                .versions
                .get(state.version)
                .map(|v| v.name.clone())
                .unwrap_or_else(|| t("common.none").into())
        } else {
            pair.get(slot).map(|a| a.name.clone()).unwrap_or_default()
        };
        let overlay = (kind == View::Review)
            .then(|| {
                let choice = pair
                    .get(slot)?
                    .metadata
                    .review
                    .as_ref()
                    .filter(|r| r.revision == pair[slot].revision)?;
                Some(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .p_2()
                        .child(t(if choice.choice == "keep" {
                            "library.similar.keep"
                        } else {
                            "library.similar.reject"
                        }))
                        .into_any_element(),
                )
            })
            .flatten();
        panes = panes.child(compare_ui::pane(
            slot,
            ComparisonPane {
                tooltip: title.clone(),
                title,
                active: state.active == slot,
                image: state.images[slot].clone(),
                preview: true,
                camera: state.camera,
                bounds: state.bounds[slot],
                message: state.error.clone().or_else(|| {
                    (pair.get(slot).is_none() && kind != View::Versions)
                        .then(|| t("common.none").into())
                }),
                culling: pair.get(slot).map(culling_value),
                overlay,
            },
            ComparisonActions::<Workspace> {
                select: |ws, slot, position, cx| {
                    ws.cloud.gallery.active = slot;
                    ws.cloud.gallery.drag = position;
                    cx.notify();
                },
                drag: |ws, position, pressed, cx| {
                    let s = &mut ws.cloud.gallery;
                    if !pressed {
                        s.drag = None;
                        return;
                    }
                    if let Some(start) = s.drag.replace(position) {
                        if let Some((_, dimensions)) = &s.images[s.active] {
                            let size = s.bounds[s.active].size;
                            let rect = s
                                .camera
                                .image_rect(*dimensions, [size.width.into(), size.height.into()]);
                            s.camera.pan(
                                [(position.x - start.x).into(), (position.y - start.y).into()],
                                [rect[2], rect[3]],
                            );
                            cx.notify();
                        }
                    }
                },
                release: |ws, _| ws.cloud.gallery.drag = None,
                zoom: |ws, factor, cx| {
                    ws.cloud.gallery.camera.zoom_by(factor);
                    cx.notify();
                },
                bounds: |ws, slot, bounds, cx| {
                    if ws.cloud.gallery.bounds[slot] != bounds {
                        ws.cloud.gallery.bounds[slot] = bounds;
                        cx.notify();
                    }
                },
            },
            cx,
        ));
    }
    let error = ws.cloud.gallery.error.clone();
    div()
        .flex()
        .flex_col()
        .flex_grow()
        .min_h(px(0.0))
        .child(header)
        .children((kind != View::Compare).then_some(bar))
        .children(error.map(|e| div().p_2().child(e)))
        .children((kind == View::Review).then(|| div().p_2().child(t("library.similar.help"))))
        .child(panes)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_edits_select_only_changed_fields_and_keep_time_modes_exclusive() {
        let mut fields = Vec::new();
        for index in 0..6 {
            fields.push((CHECK_IDS[index], String::new(), String::new()));
            fields.push((FIELD_IDS[index], String::new(), String::new()));
        }
        // Merely focusing and committing the same value must not clear metadata.
        assert!(commit_metadata_field(&mut fields, FIELD_IDS[1], ""));
        assert_eq!(metadata_enabled(&fields), [false; 6]);
        commit_metadata_field(&mut fields, FIELD_IDS[1], "Caption\nSecond line");
        assert_eq!(
            metadata_enabled(&fields),
            [false, true, false, false, false, false]
        );
        commit_metadata_field(&mut fields, FIELD_IDS[3], "2026-09-20T12:00:00Z");
        commit_metadata_field(&mut fields, FIELD_IDS[4], "3600");
        assert_eq!(
            metadata_enabled(&fields),
            [false, true, false, false, true, false]
        );
        commit_metadata_field(&mut fields, FIELD_IDS[3], "2026-09-21T12:00:00Z");
        assert_eq!(
            metadata_enabled(&fields),
            [false, true, false, true, false, false]
        );
        // Clearing a changed field is an intentional edit, too.
        commit_metadata_field(&mut fields, FIELD_IDS[1], "");
        assert!(metadata_enabled(&fields)[1]);
        assert!(!commit_metadata_field(&mut fields, "cloud-folder", "other"));
    }

    #[test]
    fn culling_filter_reset_preserves_other_cloud_search_constraints() {
        let mut filters = remote::Filters {
            person_id: Some("person".into()),
            tags: Some(vec!["holiday".into()]),
            captured_after: Some(100),
            edited: Some(true),
            ..Default::default()
        };
        let original = filters.clone();
        let selection = CullFilter {
            minimum_rating: 4,
            flag: Some(CullFlag::Pick),
            label: Some(ColourLabel::Blue),
        };
        set_culling_filter(&mut filters, selection);
        assert_eq!(culling_filter(&filters), selection);
        assert_eq!(filters.min_rating, Some(4));
        assert_eq!(filters.flag.as_deref(), Some("pick"));
        assert_eq!(filters.label.as_deref(), Some("blue"));
        set_culling_filter(&mut filters, CullFilter::default());
        assert_eq!(filters, original);
    }

    #[test]
    fn culling_filter_distinguishes_no_label_or_flag_from_all_photos() {
        let selection = CullFilter {
            flag: Some(CullFlag::None),
            label: Some(ColourLabel::None),
            ..Default::default()
        };
        let mut filters = remote::Filters::default();
        set_culling_filter(&mut filters, selection);
        assert_eq!(filters.flag.as_deref(), Some("none"));
        assert_eq!(filters.label.as_deref(), Some("none"));
        assert_eq!(culling_filter(&filters), selection);
        assert!(culling_filter(&filters).matches(PhotoCulling::default()));
        assert!(!culling_filter(&filters).matches(PhotoCulling {
            label: ColourLabel::Red,
            ..Default::default()
        }));
    }

    #[test]
    fn culling_shared_shortcuts_produce_compatible_cloud_patches() {
        for (key, field, expected) in [
            ("0", "rating", Value::from(0u8)),
            ("5", "rating", Value::from(5u8)),
            ("p", "flag", "pick".into()),
            ("x", "flag", "reject".into()),
            ("u", "flag", "none".into()),
            ("6", "label", "red".into()),
            ("7", "label", "yellow".into()),
            ("8", "label", "green".into()),
            ("9", "label", "blue".into()),
            ("m", "label", "magenta".into()),
            ("l", "label", "none".into()),
        ] {
            assert_eq!(
                culling_patch(controls_ui::shortcut(key).unwrap()),
                (field, expected)
            );
        }
        assert!(controls_ui::shortcut("a").is_none());
    }

    #[test]
    fn culling_cloud_metadata_drives_the_shared_badge_and_selection() {
        let mut asset: Asset = serde_json::from_value(serde_json::json!({
            "id": "photo", "name": "photo.jpg", "mime_type": "image/jpeg",
            "revision": 1, "size": 1, "edited": false, "tags": [],
            "rating": 0, "modified_at": 1
        }))
        .unwrap();
        assert_eq!(culling_value(&asset), PhotoCulling::default());
        assert!(badge(&asset).is_none());
        asset.metadata.label = "blue".into();
        assert_eq!(culling_value(&asset).label, ColourLabel::Blue);
        assert!(badge(&asset).is_some());
        asset.rating = 4;
        asset.metadata.flag = "pick".into();
        assert_eq!(
            culling_value(&asset),
            PhotoCulling {
                rating: 4,
                flag: CullFlag::Pick,
                label: ColourLabel::Blue,
            }
        );
    }
}
