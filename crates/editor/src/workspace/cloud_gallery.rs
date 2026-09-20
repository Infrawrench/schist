//! Cloud-backed photo decisions, metadata, review and version history.
use super::gallery_chrome::{self as chrome, pal};
use super::*;
use anyhow::{ensure, Result};
use gpui::{img, prelude::FluentBuilder};
use schist_cloud::gallery::{Target, Version};
use schist_cloud::{
    self as remote,
    protocol::{map, parse, value},
    Asset, Value,
};
use schist_gallery::{culling::CompareCamera, similar};
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
    pub controls: bool,
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
    Ok(value(&photos.iter().map(Target::from).collect::<Vec<_>>()))
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
        let (field, v) = match key {
            "0" | "1" | "2" | "3" | "4" | "5" => ("rating", Value::from(key.as_bytes()[0] - b'0')),
            "p" => ("flag", "pick".into()),
            "x" => ("flag", "reject".into()),
            "u" => ("flag", "none".into()),
            "6" => ("label", "red".into()),
            "7" => ("label", "yellow".into()),
            "8" => ("label", "green".into()),
            "9" => ("label", "blue".into()),
            "m" => ("label", "magenta".into()),
            "l" => ("label", "none".into()),
            _ => return false,
        };
        self.cloud_cull(field, v, cx);
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
const LABEL_KEYS: [&str; 6] = [
    "metadata.keywords",
    "metadata.caption",
    "metadata.copyright",
    "metadata.taken",
    "metadata.offset",
    "metadata.gps",
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
            fields.push((CHECK_IDS[i], t(LABEL_KEYS[i]).into(), String::new()));
            fields.push((FIELD_IDS[i], t(LABEL_KEYS[i]).into(), values[i].clone()));
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
                    &text
                        .split(';')
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

pub(super) fn badge(asset: &Asset) -> Option<gpui::AnyElement> {
    let flag = match asset.metadata.flag.as_str() {
        "pick" => t("culling.pick"),
        "reject" => t("culling.reject"),
        _ => "",
    };
    if asset.rating == 0
        && flag.is_empty()
        && (asset.metadata.label.is_empty() || asset.metadata.label == "none")
    {
        return None;
    }
    Some(
        div()
            .absolute()
            .bottom_1()
            .left_1()
            .px_2()
            .py_0p5()
            .rounded_sm()
            .bg(gpui::rgba(0x000000CC))
            .text_size(px(10.0))
            .text_color(gpui::rgb(0xffffff))
            .border_l_2()
            .border_color(gpui::rgb(label_colour(&asset.metadata.label)))
            .child(format!(
                "{} {flag}",
                "★".repeat(asset.rating.min(5) as usize)
            ))
            .into_any_element(),
    )
}
fn label_colour(label: &str) -> u32 {
    match label {
        "red" => 0xD84B4B,
        "yellow" => 0xD0AD25,
        "green" => 0x39AB60,
        "blue" => 0x428FD9,
        "magenta" => 0xCB50BD,
        _ => pal().text_dim,
    }
}
const FLAGS: [(&str, &str); 3] = [
    ("pick", "culling.pick"),
    ("reject", "culling.reject"),
    ("none", "common.none"),
];
const LABELS: [(&str, &str); 6] = [
    ("red", "common.red"),
    ("yellow", "common.yellow"),
    ("green", "common.green"),
    ("blue", "common.blue"),
    ("magenta", "common.magenta"),
    ("none", "common.none"),
];
pub(super) fn toolbar(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    if !feature(ws, "culling") {
        return div().into_any_element();
    }
    div()
        .when(ws.cloud.batch_cancel.is_some(), |bar| {
            bar.child(
                Button::new("cloud-batch-cancel", t("common.cancel")).on_click(cx.listener(
                    |ws, _, _, cx| {
                        if let Some(cancel) = &ws.cloud.batch_cancel {
                            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        cx.notify();
                    },
                )),
            )
        })
        .child(
            Button::new("cloud-cull-toggle", t("menu.filter"))
                .active(ws.cloud.gallery.controls)
                .on_click(cx.listener(|ws, _, _, cx| {
                    ws.cloud.gallery.controls = !ws.cloud.gallery.controls;
                    cx.notify();
                })),
        )
        .into_any_element()
}
pub(super) fn controls(ws: &mut Workspace, cx: &mut Context<Workspace>) -> gpui::AnyElement {
    let disabled = ws.cloud_gallery_photos().is_empty() || ws.cloud.gallery.busy;
    let mut edit = div()
        .flex()
        .flex_wrap()
        .gap_1()
        .items_center()
        .child(t("common.selection"));
    for r in 0..=5u8 {
        edit = edit.child(
            Button::new(("cloud-rating", r as usize), format!("{r}★"))
                .disabled(disabled)
                .on_click(cx.listener(move |ws, _, _, cx| ws.cloud_cull("rating", r.into(), cx))),
        );
    }
    for (i, (v, k)) in FLAGS.into_iter().enumerate() {
        edit = edit.child(
            Button::new(("cloud-flag", i), t(k))
                .disabled(disabled)
                .on_click(cx.listener(move |ws, _, _, cx| ws.cloud_cull("flag", v.into(), cx))),
        );
    }
    for (i, (v, k)) in LABELS.into_iter().enumerate() {
        edit = edit.child(
            Button::new(("cloud-label", i), t(k))
                .text_color(gpui::rgb(label_colour(v)))
                .disabled(disabled)
                .on_click(cx.listener(move |ws, _, _, cx| ws.cloud_cull("label", v.into(), cx))),
        );
    }
    let mut filters = div()
        .flex()
        .flex_wrap()
        .gap_1()
        .items_center()
        .child(t("menu.filter"));
    filters = filters.child(Button::new("cloud-cull-reset", t("common.all")).on_click(
        cx.listener(|ws, _, _, cx| {
            ws.cloud.query.filters.min_rating = None;
            ws.cloud.query.filters.flag = None;
            ws.cloud.query.filters.label = None;
            ws.cloud_watch_assets(true);
            cx.notify();
        }),
    ));
    for r in 1..=5u8 {
        filters = filters.child(
            Button::new(("cloud-min-rating", r as usize), format!("{r}★+"))
                .active(ws.cloud.query.filters.min_rating == Some(r))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.cloud.query.filters.min_rating =
                        if ws.cloud.query.filters.min_rating == Some(r) {
                            None
                        } else {
                            Some(r)
                        };
                    ws.cloud_watch_assets(true);
                    cx.notify();
                })),
        );
    }
    for (i, (v, k)) in FLAGS.into_iter().enumerate() {
        filters = filters.child(
            Button::new(("cloud-filter-flag", i), t(k))
                .active(ws.cloud.query.filters.flag.as_deref() == Some(v))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.cloud.query.filters.flag =
                        if ws.cloud.query.filters.flag.as_deref() == Some(v) {
                            None
                        } else {
                            Some(v.into())
                        };
                    ws.cloud_watch_assets(true);
                    cx.notify();
                })),
        );
    }
    for (i, (v, k)) in LABELS.into_iter().enumerate() {
        filters = filters.child(
            Button::new(("cloud-filter-label", i), t(k))
                .active(ws.cloud.query.filters.label.as_deref() == Some(v))
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.cloud.query.filters.label =
                        if ws.cloud.query.filters.label.as_deref() == Some(v) {
                            None
                        } else {
                            Some(v.into())
                        };
                    ws.cloud_watch_assets(true);
                    cx.notify();
                })),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .text_size(px(11.0))
        .child(edit)
        .child(filters)
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
    let mut bar = div()
        .flex()
        .flex_wrap()
        .gap_2()
        .items_center()
        .p_2()
        .child(
            Button::new("cloud-review-close", t("common.back")).on_click(cx.listener(
                |ws, _, _, cx| {
                    ws.cloud.gallery.close();
                    cx.notify();
                },
            )),
        )
        .child(t(match kind {
            View::Compare => "culling.compare",
            View::Review => "library.similar.title",
            View::Versions => "versions.title",
        }));
    for (i, (factor, label)) in [(0.8, "−"), (1.25, "+")].into_iter().enumerate() {
        bar = bar.child(
            Button::new(("cloud-compare-zoom", i), label).on_click(cx.listener(
                move |ws, _, _, cx| {
                    ws.cloud.gallery.camera.zoom_by(factor);
                    cx.notify();
                },
            )),
        );
    }
    bar = bar.child(
        Button::new("cloud-compare-fit", t("menu.view.fit_on_screen")).on_click(cx.listener(
            |ws, _, _, cx| {
                ws.cloud.gallery.camera = CompareCamera::default();
                cx.notify();
            },
        )),
    );
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
    let mut panes = div().flex().flex_row().flex_grow().min_h(px(0.0));
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
        let mut pane = div()
            .id(("cloud-compare-pane", slot))
            .relative()
            .flex_1()
            .h_full()
            .overflow_hidden()
            .bg(gpui::rgb(0x181818))
            .border_2()
            .border_color(gpui::rgb(if state.active == slot {
                0x428FD9
            } else {
                0x181818
            }));
        let entity = cx.entity();
        pane = pane.child(
            canvas(
                move |bounds, _, cx| {
                    entity.update(cx, |ws, _| ws.cloud.gallery.bounds[slot] = bounds);
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );
        if let Some((image, dimensions)) = &state.images[slot] {
            let bounds = state.bounds[slot];
            let [x, y, w, h] = state.camera.image_rect(
                *dimensions,
                [f32::from(bounds.size.width), f32::from(bounds.size.height)],
            );
            pane = pane.child(
                img(image.clone())
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(w))
                    .h(px(h)),
            );
        } else if state.busy || !title.is_empty() {
            pane = pane.child(div().p_4().child(t("versions.loading")));
        }
        pane = pane.child(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .p_2()
                .bg(gpui::rgba(0x000000cc))
                .child(title),
        );
        if let Some(a) = pair.get(slot) {
            pane = pane.children(badge(a));
            if kind == View::Review {
                if let Some(choice) = a
                    .metadata
                    .review
                    .as_ref()
                    .filter(|r| r.revision == a.revision)
                {
                    pane = pane.child(div().absolute().top_0().right_0().p_2().child(t(
                        if choice.choice == "keep" {
                            "library.similar.keep"
                        } else {
                            "library.similar.reject"
                        },
                    )));
                }
            }
        }
        pane = pane
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, e: &MouseDownEvent, _, cx| {
                    ws.cloud.gallery.active = slot;
                    ws.cloud.gallery.drag = Some(e.position);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|ws, _, _, _| ws.cloud.gallery.drag = None),
            )
            .on_mouse_move(cx.listener(move |ws, e: &MouseMoveEvent, _, cx| {
                let s = &mut ws.cloud.gallery;
                if e.pressed_button != Some(MouseButton::Left) {
                    s.drag = None;
                    return;
                }
                if let Some(start) = s.drag.replace(e.position) {
                    if let Some((_, dims)) = &s.images[slot] {
                        let b = s.bounds[slot];
                        let r = s
                            .camera
                            .image_rect(*dims, [f32::from(b.size.width), f32::from(b.size.height)]);
                        s.camera.pan(
                            [
                                f32::from(e.position.x - start.x),
                                f32::from(e.position.y - start.y),
                            ],
                            [r[2], r[3]],
                        );
                        cx.notify();
                    }
                }
            }));
        panes = panes.child(pane);
    }
    let error = ws.cloud.gallery.error.clone();
    div()
        .flex()
        .flex_col()
        .flex_grow()
        .min_h(px(0.0))
        .child(bar)
        .children(error.map(|e| div().p_2().child(e)))
        .children((kind == View::Review).then(|| div().p_2().child(t("library.similar.help"))))
        .child(panes)
        .into_any_element()
}
