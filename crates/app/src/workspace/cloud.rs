//! Cloud account, live queries and the editor/provider binding. The socket lives
//! in schist-cloud; this module translates its events into GPUI state changes.
use super::gallery_chrome::GridScroll;
#[cfg(target_arch = "wasm32")]
use super::gallery_chrome::GroupBy;
use super::*;
use crate::ui::LineEdit;
use anyhow::{anyhow, Result};
use remote::transfer::DownloadedAsset;
use schist_cloud::{
    self as remote,
    protocol::{bytes, map, parse, value},
    Account, Asset, AssetQuery, Bucket, CatalogueQuery, Client, Event, Filters, Folder, Scope,
    Value, WatchQuery,
};
use schist_core::DocumentId;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

#[cfg(not(target_arch = "wasm32"))]
const CREDENTIAL_KEY: &str = "https://schist.app/schist-cloud";
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn state_dir() -> PathBuf {
    schist_gallery::state_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("schist/cloud")
}

pub(crate) struct RemoteDocument {
    pub asset: Asset,
    pub shared: remote::document::SharedDocument,
    pub joined: bool,
    pub detached: bool,
    pub vector: Vec<u8>,
    pub sending: bool,
    pub changed: bool,
    pub generation: u64,
    pub saved: u64,
    pub render: bool,
}
enum Pending {
    Capabilities,
    Join(DocumentId),
    Update {
        doc: DocumentId,
        vector: Vec<u8>,
        generation: u64,
    },
    Mutation,
}
impl Pending {
    fn belongs_to(&self, id: DocumentId) -> bool {
        matches!(self, Self::Join(doc) | Self::Update { doc, .. } if *doc == id)
    }
}
impl RemoteDocument {
    fn detach(&mut self) {
        self.detached = true;
        self.joined = false;
        self.sending = false;
        self.render = false;
    }
}
#[cfg(not(target_arch = "wasm32"))]
enum RecoveryTask {
    Write {
        epoch: u64,
        files: Vec<(PathBuf, Vec<u8>)>,
    },
    Remove(PathBuf),
}
enum Job {
    Thumbnail {
        epoch: u64,
        id: String,
        revision: u64,
        image: Option<Arc<RenderImage>>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    Browser {
        epoch: u64,
        url: String,
    },
    SignedIn {
        epoch: u64,
        account: Account,
    },
    Error {
        epoch: u64,
        error: String,
    },
    Opened {
        epoch: u64,
        asset: Asset,
        download: DownloadedAsset,
    },
    Downloaded {
        epoch: u64,
        name: String,
        download: DownloadedAsset,
    },
    Uploaded {
        epoch: u64,
        doc: DocumentId,
        asset: Asset,
    },
    Done {
        epoch: u64,
        message: String,
    },
    /// A long transfer's progress, for the tray's bar.
    Progress {
        epoch: u64,
        done: u64,
        total: u64,
        label: String,
    },
    /// A bucket's originals landed in a scratch folder for the batch
    /// dialog.
    #[cfg(not(target_arch = "wasm32"))]
    Batch {
        epoch: u64,
        paths: Vec<PathBuf>,
    },
    /// The world map's located assets for one query.
    #[cfg(not(target_arch = "wasm32"))]
    MapAssets {
        epoch: u64,
        key: (AssetQuery, u64),
        result: std::result::Result<Vec<Asset>, String>,
    },
}
/// What the cloud gallery's right-click menu is about.
#[derive(Clone, Debug)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) enum CloudContext {
    Photo(String),
    Folder(String),
    Bucket(String),
    /// A world-map marker's photos.
    Cluster(Vec<String>),
    /// The ☁ Schist Cloud root row.
    Library,
    /// A named person in the cloud's PEOPLE list.
    Person(String),
}
/// Assets per page. A page's thumbnails stay decoded while it shows,
/// so this bounds texture memory as much as it bounds the query.
pub(crate) const PAGE_SIZE: u64 = 200;
/// Concurrent thumbnail fetches, and how many decoded thumbnails stay
/// in memory (~256 KB each) before the cache shrinks to the page.
const THUMBNAIL_WORKERS: usize = 8;
const THUMBNAIL_CACHE: usize = 600;
pub(crate) struct CloudState {
    pub generation: super::cloud_generation::GenerationState,
    pub account: Option<Account>,
    pub client: Option<Client>,
    pub connected: bool,
    pub capabilities: Option<remote::Capabilities>,
    pub capabilities_ready: bool,
    pub download_target: Option<Asset>,
    pub show: bool,
    pub message: String,
    pub thumbnails: HashMap<String, (u64, Arc<RenderImage>)>,
    thumbnail_jobs: HashSet<(String, u64)>,
    thumbnail_active: usize,
    pub folders: Vec<Folder>,
    pub buckets: Vec<Bucket>,
    pub assets: Vec<Asset>,
    pub total: u64,
    pub query: AssetQuery,
    /// The selected assets, in the order they were picked; the last is
    /// the lead — what arrows move and Enter opens.
    pub selected: Vec<String>,
    /// Where a Shift-click range extends from.
    pub select_anchor: Option<String>,
    /// The search box in the top strip; its text becomes the query
    /// after a short pause in typing.
    pub search: LineEdit,
    pub search_seq: u64,
    /// The grid's scroll and viewport bookkeeping.
    pub grid: GridScroll,
    /// The gallery's right-click menu: where, and on what.
    pub context: Option<(Point<Pixels>, CloudContext)>,
    /// Whether the current asset watch has delivered its first snapshot.
    pub loaded: bool,
    /// A failed watch is unavailable, not an empty library or an ongoing load.
    pub load_error: Option<String>,
    /// "Select all" asked before the page arrived: select it on landing.
    pub select_all_pending: bool,
    /// The photos of the world-map marker last clicked, for its strip.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub map_photos: Vec<String>,
    /// Every located photo in the scope on show, for the world map —
    /// the whole scope, not one page — and the query plus change count
    /// it answers, so it refetches only when either moves.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub map_assets: Vec<Asset>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub map_key: Option<(AssetQuery, u64)>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub map_loading: bool,
    /// Marker previews asked for this frame: their thumbnails load
    /// alongside the page's.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub map_wanted: HashSet<String>,
    /// Bumped on every asset snapshot, so caches keyed by it refresh.
    pub changes: u64,
    /// A transfer under way: done, total, and what it is doing — the
    /// tray draws a bar from it in either room.
    pub progress: Option<(u64, u64, String)>,
    /// Thumbnails whose fetch or decode failed at the current revision.
    pub thumbnail_failed: HashSet<String>,
    /// The browser has no local gallery to keep these in.
    #[cfg(target_arch = "wasm32")]
    pub thumb_px: f32,
    #[cfg(target_arch = "wasm32")]
    pub group_by: GroupBy,
    pub catalogue: String,
    pub folders_offset: u64,
    pub buckets_offset: u64,
    pub folders_total: u64,
    pub library_total: Option<u64>,
    pub screening: remote::Screening,
    pub people: Option<remote::People>,
    pub face_bounds: Bounds<Pixels>,
    pub face_start: Option<(f32, f32)>,
    pub face_draft: Option<remote::FaceRect>,
    pub face_drawing: bool,
    pub buckets_total: u64,
    pub docs: HashMap<DocumentId, RemoteDocument>,
    pending: HashMap<String, Pending>,
    pub epoch: u64,
    jobs: mpsc::Receiver<Job>,
    sender: mpsc::Sender<Job>,
    cancel: Arc<AtomicBool>,
    writes: VecDeque<Option<Account>>,
    #[cfg(not(target_arch = "wasm32"))]
    writing: bool,
    #[cfg(not(target_arch = "wasm32"))]
    recovery: mpsc::Sender<RecoveryTask>,
    pub watching: String,
    folders_watch: String,
    buckets_watch: String,
    pub form_target: Option<(String, u64)>,
    pub form_scope: Scope,
}
impl Default for CloudState {
    fn default() -> Self {
        let (sender, jobs) = mpsc::channel();
        #[cfg(not(target_arch = "wasm32"))]
        let recovery = {
            let (recovery, tasks) = mpsc::channel();
            let errors = sender.clone();
            std::thread::spawn(move || {
                while let Ok(task) = tasks.recv() {
                    match task {
                        RecoveryTask::Write { epoch, files } => {
                            for (path, bytes) in files {
                                if let Err(e) = remote::auth::private_write(&path, &bytes) {
                                    let _ = errors.send(Job::Error {
                                        epoch,
                                        error: format!("Cloud recovery failed: {e}"),
                                    });
                                }
                            }
                        }
                        RecoveryTask::Remove(path) => {
                            let _ = std::fs::remove_file(path);
                        }
                    }
                }
            });
            recovery
        };
        Self {
            generation: Default::default(),
            account: None,
            client: None,
            connected: false,
            capabilities: None,
            capabilities_ready: false,
            download_target: None,
            show: false,
            message: "Not signed in".into(),
            thumbnails: HashMap::new(),
            thumbnail_jobs: HashSet::new(),
            thumbnail_active: 0,
            folders: vec![],
            buckets: vec![],
            assets: vec![],
            total: 0,
            query: AssetQuery {
                limit: PAGE_SIZE,
                ..Default::default()
            },
            selected: Vec::new(),
            select_anchor: None,
            search: LineEdit::default(),
            search_seq: 0,
            grid: GridScroll::default(),
            context: None,
            loaded: false,
            load_error: None,
            select_all_pending: false,
            map_photos: Vec::new(),
            map_assets: Vec::new(),
            map_key: None,
            map_loading: false,
            map_wanted: HashSet::new(),
            changes: 0,
            progress: None,
            thumbnail_failed: HashSet::new(),
            #[cfg(target_arch = "wasm32")]
            thumb_px: 144.0,
            #[cfg(target_arch = "wasm32")]
            group_by: GroupBy::Date,
            catalogue: String::new(),
            folders_offset: 0,
            buckets_offset: 0,
            folders_total: 0,
            library_total: None,
            screening: remote::Screening::default(),
            people: None,
            face_bounds: Bounds::default(),
            face_start: None,
            face_draft: None,
            face_drawing: false,
            buckets_total: 0,
            docs: HashMap::new(),
            pending: HashMap::new(),
            epoch: 0,
            jobs,
            sender,
            cancel: Arc::new(AtomicBool::new(false)),
            writes: VecDeque::new(),
            #[cfg(not(target_arch = "wasm32"))]
            writing: false,
            #[cfg(not(target_arch = "wasm32"))]
            recovery,
            watching: String::new(),
            folders_watch: String::new(),
            buckets_watch: String::new(),
            form_target: None,
            form_scope: Scope::Library,
        }
    }
}
impl Drop for CloudState {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl CloudState {
    pub(crate) fn is_loading(&self) -> bool {
        self.account.is_some() && !self.loaded && self.load_error.is_none()
    }
    fn joinable_documents(&self) -> Vec<DocumentId> {
        self.docs
            .iter()
            .filter(|(_, doc)| !doc.detached)
            .map(|(id, _)| *id)
            .collect()
    }
    fn disconnect(&mut self) {
        self.connected = false;
        self.capabilities_ready = false;
        for document in self.docs.values_mut() {
            document.joined = false;
            document.sending = false;
        }
        self.pending.clear();
    }
    fn detach_document(&mut self, asset: &str) -> Option<DocumentId> {
        let id = self
            .docs
            .iter()
            .find(|(_, document)| document.asset.id == asset)
            .map(|(id, _)| *id)?;
        self.docs.get_mut(&id)?.detach();
        self.pending.retain(|_, pending| !pending.belongs_to(id));
        Some(id)
    }
    fn apply_document_update(&mut self, asset: &str, update: &[u8]) -> Result<()> {
        if let Some(document) = self
            .docs
            .values_mut()
            .find(|doc| doc.asset.id == asset && !doc.detached)
        {
            document.shared.apply(update)?;
            document.render = true;
        }
        Ok(())
    }
    fn reopen_document(&mut self, id: DocumentId) -> bool {
        let Some(document) = self.docs.get_mut(&id) else {
            return false;
        };
        std::mem::replace(&mut document.detached, false)
    }
}
impl Workspace {
    pub(crate) fn cloud_start(&mut self, cx: &mut Context<Self>) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let read = cx.read_credentials(CREDENTIAL_KEY);
            let epoch = self.cloud.epoch;
            cx.spawn(async move |this, cx| {
                let result = read.await;
                let _ = this.update(cx, |ws, cx| {
                    if ws.cloud.epoch != epoch {
                        return;
                    }
                    match result {
                        Ok(Some((_, data))) => match serde_json::from_slice::<Account>(&data) {
                            Ok(account) => ws.cloud_connect(account, cx),
                            Err(e) => ws.cloud_error(format!("Stored cloud login is invalid: {e}")),
                        },
                        Ok(None) => {}
                        Err(e) => ws.cloud_error(format!("Could not read cloud login: {e}")),
                    }
                });
            })
            .detach();
        }
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(75))
                .await;
            if this.update(cx, |ws, cx| ws.cloud_tick(cx)).is_err() {
                break;
            }
        })
        .detach();
    }
    fn cloud_error(&mut self, error: impl Into<String>) {
        let error = error.into();
        self.status = error.clone().into();
        self.cloud.message = error;
    }
    pub(crate) fn cloud_sign_in(&mut self, cx: &mut Context<Self>) {
        #[cfg(target_arch = "wasm32")]
        {
            self.cloud_login("https://schist.app".into(), cx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.open_modal(
                Modal::Cloud {
                    kind: "sign-in",
                    fields: vec![(
                        "cloud-domain",
                        "Domain".into(),
                        remote::DEFAULT_DOMAIN.into(),
                    )],
                },
                cx,
            );
            self.focus_field("cloud-domain", remote::DEFAULT_DOMAIN);
        }
    }
    fn cloud_login(&mut self, domain: String, cx: &mut Context<Self>) {
        self.cloud.epoch += 1;
        let epoch = self.cloud.epoch;
        self.cloud.cancel.store(true, Ordering::Relaxed);
        self.cloud.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cloud.cancel.clone();
        let sender = self.cloud.sender.clone();
        self.cloud.message = "Opening sign-in in your browser…".into();
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || {
            let result = (|| -> Result<()> {
                let login = remote::auth::Login::discover(&domain, &state_dir())?;
                let _ = sender.send(Job::Browser {
                    epoch,
                    url: login.browser_url.clone(),
                });
                while !cancel.load(Ordering::Relaxed) {
                    if let Some(callback) = login.poll()? {
                        match login.exchange(&callback) {
                            Ok(account) => {
                                let _ = sender.send(Job::SignedIn { epoch, account });
                                return Ok(());
                            }
                            Err(e) => {
                                let _ = sender.send(Job::Error {
                                    epoch,
                                    error: e.to_string(),
                                });
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Ok(())
            })();
            if let Err(e) = result {
                let _ = sender.send(Job::Error {
                    epoch,
                    error: format!("Cloud sign-in failed: {e}"),
                });
            }
        });
        #[cfg(target_arch = "wasm32")]
        {
            let result = remote::auth::domain(&domain).and_then(|_| remote::auth::Login::open());
            match result {
                Ok(login) => remote::runtime::spawn(async move {
                    let job = match login.finish(&cancel).await {
                        Ok(account) => Job::SignedIn { epoch, account },
                        Err(e) => Job::Error {
                            epoch,
                            error: e.to_string(),
                        },
                    };
                    let _ = sender.send(job);
                }),
                Err(e) => self.cloud_error(e.to_string()),
            }
        }
        cx.notify();
    }
    fn cloud_connect(&mut self, account: Account, cx: &mut Context<Self>) {
        self.cloud.library_total = None;
        self.cloud.client = Some(Client::start(account.clone()));
        self.cloud.account = Some(account.clone());
        self.cloud.writes.push_back(Some(account));
        self.cloud.message = "Connecting to Schist Cloud…".into();
        self.cloud.pending.clear();
        self.cloud_refresh_catalogue();
        // Connect quietly: the cloud's rows appear in the sidebar, but
        // whatever screen is up stays up — a stored login must not pull
        // an open image out from under its editor.
        self.cloud.query.scope = Scope::Library;
        self.cloud.query.offset = 0;
        self.cloud_watch_assets(false);
        cx.notify();
    }
    pub(crate) fn cloud_sign_out(&mut self, cx: &mut Context<Self>) {
        self.cloud_capture_edit();
        self.cloud_checkpoint();
        self.cloud.epoch += 1;
        self.cloud.cancel.store(true, Ordering::Relaxed);
        self.cloud.generation.cancel.store(true, Ordering::Relaxed);
        self.cloud.generation = Default::default();
        let account = self.cloud.account.take();
        self.cloud.client = None;
        self.cloud.connected = false;
        self.cloud.capabilities = None;
        self.cloud.capabilities_ready = false;
        self.cloud.download_target = None;
        self.cloud.show = false;
        self.cloud.pending.clear();
        self.cloud.docs.clear();
        self.cloud.assets.clear();
        self.cloud.thumbnails.clear();
        self.cloud.library_total = None;
        self.cloud.thumbnail_jobs.clear();
        self.cloud.thumbnail_active = 0;
        self.cloud.thumbnail_failed.clear();
        self.cloud.selected.clear();
        self.cloud.select_anchor = None;
        self.cloud.search.clear();
        self.cloud.context = None;
        self.cloud.folders.clear();
        self.cloud.buckets.clear();
        self.cloud.writes.push_back(None);
        self.cloud.message = "Signed out".into();
        if let Some(account) = account {
            let sender = self.cloud.sender.clone();
            let epoch = self.cloud.epoch;
            remote::runtime::spawn(async move {
                if let Err(e) = remote::auth::logout_async(&account).await {
                    let _ = sender.send(Job::Error {
                        epoch,
                        error: format!("Signed out locally; server logout failed: {e}"),
                    });
                }
            });
        }
        cx.notify();
    }
    pub(crate) fn cloud_set_visible(&mut self, visible: bool) {
        self.cloud.show = visible;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.library.open = visible;
        }
    }
    pub(crate) fn cloud_browse(&mut self, scope: Scope, cx: &mut Context<Self>) {
        if self.cloud.account.is_none() {
            self.cloud_sign_in(cx);
            return;
        }
        self.cloud.show = true;
        self.cloud_set_visible(true);
        self.cloud.query.scope = scope;
        self.cloud.query.offset = 0;
        self.cloud.selected.clear();
        self.cloud.select_anchor = None;
        self.cloud.context = None;
        self.cloud_watch_assets(false);
        cx.notify();
    }
    /// (Re)subscribe to the assets the query names. `keep` leaves the
    /// current page on screen until the new one lands — what a search
    /// refinement wants; a change of scope starts from a blank grid.
    pub(crate) fn cloud_watch_assets(&mut self, keep: bool) {
        self.cloud.query.sort = self.cloud_sort();
        self.cloud.query.limit = PAGE_SIZE;
        self.cloud.loaded = false;
        self.cloud.load_error = None;
        if !keep {
            self.cloud.assets.clear();
            self.cloud.total = 0;
            self.cloud.grid.handle.set_offset(point(px(0.0), px(0.0)));
        }
        if let Some(c) = &self.cloud.client {
            if !self.cloud.watching.is_empty() {
                c.handle.unwatch(&self.cloud.watching);
            }
            self.cloud.watching = remote::Uuid::new_v4().to_string();
            c.handle.watch(
                &self.cloud.watching,
                WatchQuery::Assets {
                    query: Box::new(self.cloud.query.clone()),
                },
            );
        }
    }
    pub(crate) fn cloud_refresh_catalogue(&mut self) {
        if let Some(c) = &self.cloud.client {
            c.handle.unwatch(&self.cloud.folders_watch);
            c.handle.unwatch(&self.cloud.buckets_watch);
            self.cloud.folders_watch = remote::Uuid::new_v4().to_string();
            self.cloud.buckets_watch = remote::Uuid::new_v4().to_string();
            c.handle.watch(
                &self.cloud.folders_watch,
                WatchQuery::Folders {
                    query: CatalogueQuery {
                        text: self.cloud.catalogue.clone(),
                        offset: self.cloud.folders_offset,
                        limit: 500,
                    },
                },
            );
            c.handle.watch(
                &self.cloud.buckets_watch,
                WatchQuery::Buckets {
                    query: CatalogueQuery {
                        text: self.cloud.catalogue.clone(),
                        offset: self.cloud.buckets_offset,
                        limit: 500,
                    },
                },
            );
        }
    }
    fn cloud_load_thumbnails(&mut self) {
        if !self.cloud.show {
            return;
        }
        // A small worker pool bounds the network; decoded thumbnails
        // stay for a few pages, so paging back is instant, and only an
        // overfull cache falls back to the page on show.
        // The page's photos, then whatever the world map's markers asked
        // for this frame.
        let wanted: Vec<(String, u64, Option<String>)> = self
            .cloud
            .assets
            .iter()
            .chain(
                self.cloud
                    .map_assets
                    .iter()
                    .filter(|a| self.cloud.map_wanted.contains(&a.id)),
            )
            .map(|a| (a.id.clone(), a.revision, a.thumbnail_url.clone()))
            .collect();
        let visible: HashSet<String> = wanted.iter().map(|(id, _, _)| id.clone()).collect();
        if self.cloud.thumbnails.len() > THUMBNAIL_CACHE {
            self.cloud.thumbnails.retain(|id, _| visible.contains(id));
        }
        self.cloud
            .thumbnail_jobs
            .retain(|(id, _)| visible.contains(id));
        self.cloud
            .thumbnail_failed
            .retain(|id| visible.contains(id));
        for (id, revision, url) in wanted {
            if self.cloud.thumbnail_active >= THUMBNAIL_WORKERS {
                break;
            }
            if self
                .cloud
                .thumbnails
                .get(&id)
                .is_some_and(|(r, _)| *r == revision)
            {
                continue;
            }
            let Some(url) = url else {
                continue;
            };
            if !self.cloud.thumbnail_jobs.insert((id.clone(), revision)) {
                continue;
            }
            self.cloud.thumbnail_active += 1;
            let sender = self.cloud.sender.clone();
            let epoch = self.cloud.epoch;
            remote::runtime::spawn(async move {
                let image = (async {
                    let bytes = remote::auth::download_limited_async(&url, 8 * 1024 * 1024).await?;
                    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
                        .with_guessed_format()?;
                    let mut limits = image::Limits::default();
                    limits.max_image_width = Some(4096);
                    limits.max_image_height = Some(4096);
                    limits.max_alloc = Some(64 * 1024 * 1024);
                    reader.limits(limits);
                    let image = reader.decode()?.thumbnail(256, 256).to_rgba8();
                    rgba_to_render_image(image.width(), image.height(), image.into_raw())
                        .ok_or_else(|| anyhow!("Invalid thumbnail"))
                })
                .await
                .map_err(|e| log::warn!("cloud: thumbnail for {id} failed: {e}"))
                .ok();
                let _ = sender.send(Job::Thumbnail {
                    epoch,
                    id,
                    revision,
                    image,
                });
            });
        }
    }
    fn cloud_tick(&mut self, cx: &mut Context<Self>) {
        self.cloud_capture_edit();
        self.cloud_generation_tick(cx);
        let mut changed = false;
        while let Ok(job) = self.cloud.jobs.try_recv() {
            changed = true;
            match job {
                Job::Thumbnail {
                    epoch,
                    id,
                    revision,
                    image,
                } if epoch == self.cloud.epoch => {
                    self.cloud.thumbnail_active = self.cloud.thumbnail_active.saturating_sub(1);
                    match image {
                        Some(image) => {
                            self.cloud.thumbnail_failed.remove(&id);
                            self.cloud.thumbnails.insert(id, (revision, image));
                        }
                        None => {
                            self.cloud.thumbnail_failed.insert(id);
                        }
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                Job::Browser { epoch, url } if epoch == self.cloud.epoch => cx.open_url(&url),
                Job::SignedIn { epoch, account } if epoch == self.cloud.epoch => {
                    self.cloud_connect(account, cx)
                }
                Job::Error { epoch, error } if epoch == self.cloud.epoch => {
                    self.cloud.progress = None;
                    if error.starts_with("Not enough cloud storage.") {
                        self.open_modal(
                            Modal::Cloud {
                                kind: "storage-warning",
                                fields: vec![("message", String::new(), error.clone())],
                            },
                            cx,
                        );
                    }
                    self.cloud_error(error)
                }
                Job::Done { epoch, message } if epoch == self.cloud.epoch => {
                    self.cloud.progress = None;
                    self.status = message.clone().into();
                    self.cloud.message = message;
                }
                Job::Progress {
                    epoch,
                    done,
                    total,
                    label,
                } if epoch == self.cloud.epoch => {
                    self.status = label.clone().into();
                    self.cloud.message = label.clone();
                    self.cloud.progress = Some((done, total, label));
                }
                #[cfg(not(target_arch = "wasm32"))]
                Job::MapAssets { epoch, key, result } if epoch == self.cloud.epoch => {
                    self.cloud.map_loading = false;
                    // A failed fetch still records its key, so the map
                    // does not ask again every frame; the next change
                    // or scope retries.
                    self.cloud.map_key = Some(key);
                    match result {
                        Ok(assets) => self.cloud.map_assets = assets,
                        Err(error) => self.cloud_error(format!("World map: {error}")),
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                Job::Batch { epoch, paths } if epoch == self.cloud.epoch => {
                    self.open_batch_process(paths, cx);
                    self.update_modal(|m| {
                        if let Modal::BatchProcess { target, .. } = m {
                            *target = super::BatchTarget::Folder;
                        }
                    });
                }
                Job::Opened {
                    epoch,
                    mut asset,
                    download,
                } if epoch == self.cloud.epoch => {
                    asset.name = download.suggested_name(&asset.name);
                    asset.revision = download.revision;
                    if let Some(mime) = &download.content_type {
                        asset.mime_type = mime.clone();
                    }
                    if let Err(e) = self.cloud_install(asset, download.bytes, cx) {
                        self.cloud_error(format!("Open cloud document: {e}"));
                    }
                }
                Job::Downloaded {
                    epoch,
                    name,
                    download,
                } if epoch == self.cloud.epoch => {
                    self.cloud_save_download(name, download, cx);
                }
                Job::Uploaded { epoch, doc, asset } if epoch == self.cloud.epoch => {
                    if asset
                        .moderation
                        .as_ref()
                        .is_some_and(|m| m.status != "clear")
                    {
                        self.status = "Uploaded — screening before it appears in Schist Cloud. This document remains local; open its cloud copy after screening.".into();
                        continue;
                    }
                    if let Some(source) = self.cloud_doc(doc) {
                        match remote::document::SharedDocument::unseeded(source) {
                            Ok(shared) => {
                                self.cloud_close_document(doc);
                                self.cloud.docs.insert(
                                    doc,
                                    RemoteDocument {
                                        asset,
                                        shared,
                                        joined: false,
                                        detached: false,
                                        vector: vec![0],
                                        sending: false,
                                        changed: true,
                                        generation: 0,
                                        saved: 0,
                                        render: false,
                                    },
                                );
                                self.cloud_join(doc);
                            }
                            Err(e) => self.cloud_error(e.to_string()),
                        }
                    }
                }
                _ => {}
            }
        }
        let events: Vec<_> = self
            .cloud
            .client
            .as_ref()
            .map(|c| c.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            changed = true;
            if let Err(e) = self.cloud_event(event, cx) {
                self.cloud_error(e.to_string());
            }
        }
        // Capture before applying remote state, so local changes made in the same UI tick survive.
        self.cloud_capture_edit();
        if !self.pointer_down && self.modal.is_none() {
            let ids: Vec<_> = self
                .cloud
                .docs
                .iter()
                .filter(|(_, d)| d.render)
                .map(|(id, _)| *id)
                .collect();
            for id in ids {
                if let Err(e) = self.cloud_render_document(id) {
                    self.cloud_error(e.to_string());
                }
                changed = true;
            }
        }
        let ids: Vec<_> = self.cloud.docs.keys().copied().collect();
        for id in ids {
            self.cloud_send_document(id);
        }
        self.cloud_load_thumbnails();
        self.cloud_persist_credentials(cx);
        if changed {
            cx.notify();
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn cloud_persist_credentials(&mut self, cx: &mut Context<Self>) {
        if self.cloud.writing {
            return;
        }
        let Some(account) = self.cloud.writes.pop_front() else {
            return;
        };
        self.cloud.writing = true;
        let task = match account {
            Some(account) => match serde_json::to_vec(&account) {
                Ok(data) => cx.write_credentials(CREDENTIAL_KEY, &account.domain, &data),
                Err(e) => {
                    self.cloud.writing = false;
                    self.cloud_error(e.to_string());
                    return;
                }
            },
            None => cx.delete_credentials(CREDENTIAL_KEY),
        };
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |ws, cx| {
                ws.cloud.writing = false;
                if let Err(e) = result {
                    ws.cloud_error(format!("Could not persist cloud login: {e}"));
                }
                cx.notify();
            });
        })
        .detach();
    }
    #[cfg(target_arch = "wasm32")]
    fn cloud_persist_credentials(&mut self, _: &mut Context<Self>) {
        self.cloud.writes.clear();
    }
    fn cloud_event(&mut self, event: Event, cx: &mut Context<Self>) -> Result<()> {
        match event {
            Event::Connected => {
                self.cloud.connected = true;
                self.cloud.load_error = None;
                self.cloud.capabilities = None;
                self.cloud.capabilities_ready = false;
                self.cloud.message = "Connected to Schist Cloud".into();
                if let Some(client) = &self.cloud.client {
                    let id = client.handle.call("workspace.capabilities", map([]));
                    self.cloud.pending.insert(id, Pending::Capabilities);
                }
            }
            Event::AccountUnavailable => {
                self.cloud.disconnect();
                self.cloud.epoch += 1;
                self.cloud.account = None;
                self.cloud.client = None;
                self.cloud.writes.push_back(None);
                self.cloud.assets.clear();
                self.cloud.folders.clear();
                self.cloud.buckets.clear();
                self.cloud.thumbnails.clear();
                self.cloud.selected.clear();
                self.cloud.people = None;
                self.cloud.total = 0;
                self.cloud.library_total = None;
                for doc in self.cloud.docs.values_mut() {
                    doc.detach();
                }
                self.cloud_error("Cloud access is unavailable. Sign in again to check your account; local files remain on this device.");
            }
            Event::Disconnected(error) => {
                self.cloud.disconnect();
                if !self.cloud.loaded {
                    self.cloud.load_error = Some(error.clone());
                }
                self.cloud.message = error;
            }
            Event::Credentials(account) => {
                self.cloud.account = Some(account.clone());
                self.cloud.writes.push_back(Some(account));
            }
            Event::Snapshot {
                subscription_id,
                snapshot,
            } => {
                if let Some(screening) = snapshot.screening {
                    self.cloud.screening = screening;
                }
                if subscription_id == self.cloud.watching {
                    self.cloud.people = snapshot.people;
                }
                match subscription_id.as_str() {
                    id if id == self.cloud.folders_watch => {
                        self.cloud.library_total = snapshot.library_asset_count;
                        self.cloud.folders = snapshot
                            .items
                            .into_iter()
                            .map(parse)
                            .collect::<Result<_>>()?;
                        self.cloud.folders_total = snapshot.total;
                    }
                    id if id == self.cloud.buckets_watch => {
                        self.cloud.buckets = snapshot
                            .items
                            .into_iter()
                            .map(parse)
                            .collect::<Result<_>>()?;
                        self.cloud.buckets_total = snapshot.total;
                    }
                    id if id == self.cloud.watching => {
                        self.cloud.assets = snapshot
                            .items
                            .into_iter()
                            .map(parse)
                            .collect::<Result<_>>()?;
                        self.cloud.total = snapshot.total;
                        self.cloud.loaded = true;
                        self.cloud.load_error = None;
                        self.cloud.changes += 1;
                        if std::mem::take(&mut self.cloud.select_all_pending) {
                            self.cloud.selected = self.cloud_flat_order();
                            self.cloud.select_anchor = self.cloud.selected.first().cloned();
                        }
                        let ids: HashSet<_> =
                            self.cloud.assets.iter().map(|a| a.id.clone()).collect();
                        self.cloud.selected.retain(|id| ids.contains(id));
                        if self
                            .cloud
                            .select_anchor
                            .as_ref()
                            .is_some_and(|id| !ids.contains(id))
                        {
                            self.cloud.select_anchor = None;
                        }
                    }
                    _ => {}
                }
            }
            Event::WatchError {
                subscription_id,
                error,
            } => {
                if subscription_id == self.cloud.watching {
                    self.cloud.assets.clear();
                    self.cloud.total = 0;
                    self.cloud.loaded = false;
                    self.cloud.load_error = Some(error.clone());
                }
                self.cloud_error(error);
            }
            Event::DocumentUpdate { asset_id, bytes } => {
                self.cloud.apply_document_update(&asset_id, &bytes)?;
            }
            Event::DocumentError { asset_id, error } => {
                if let Some(id) = self.cloud.detach_document(&asset_id) {
                    if self.close_after_save == Some(id) {
                        self.close_after_save = None;
                        self.cancel_quit();
                    }
                }
                self.cloud_error(error);
            }
            Event::Reply { id, result } => {
                let Some(p) = self.cloud.pending.remove(&id) else {
                    return Ok(());
                };
                match (p, result) {
                    (Pending::Capabilities, result) => {
                        let capabilities = remote::Capabilities::from_reply(result)?;
                        if let Some(client) = &self.cloud.client {
                            client.handle.set_frame_limit(
                                capabilities
                                    .as_ref()
                                    .map_or(remote::MAX_FRAME, |c| c.frame_limit()),
                            );
                        }
                        self.cloud.capabilities = capabilities;
                        self.cloud.capabilities_ready = true;
                        let ids = self.cloud.joinable_documents();
                        for id in ids {
                            self.cloud_join(id);
                        }
                    }
                    (Pending::Join(id), Ok(result)) => {
                        if let Some(d) = self.cloud.docs.get_mut(&id) {
                            d.shared.apply(&bytes(&result, "update")?)?;
                            d.shared.seed_if_empty()?;
                            d.vector = bytes(&result, "state_vector")?;
                            d.joined = true;
                            d.changed = true;
                            d.render = true;
                        }
                    }
                    (
                        Pending::Update {
                            doc,
                            vector,
                            generation,
                        },
                        Ok(_),
                    ) => {
                        if let Some(d) = self.cloud.docs.get_mut(&doc) {
                            d.sending = false;
                            d.vector = vector;
                            d.saved = generation;
                            let saved = d.saved == d.generation;
                            if saved {
                                if let Some(doc) = self.cloud_doc_mut(doc) {
                                    doc.mark_saved();
                                }
                            }
                        }
                        if self.close_after_save == Some(doc)
                            && self
                                .cloud
                                .docs
                                .get(&doc)
                                .is_some_and(|d| d.saved == d.generation)
                        {
                            self.cloud_finish_save(doc, cx);
                        }
                    }
                    (Pending::Update { doc, .. }, Err(error)) => {
                        if let Some(d) = self.cloud.docs.get_mut(&doc) {
                            d.sending = false;
                            d.joined = false;
                        }
                        self.cloud_error(error);
                    }
                    (_, Err(error)) => self.cloud_error(error),
                    (_, Ok(_)) => {
                        self.cloud.message = "Cloud updated".into();
                    }
                }
            }
        }
        Ok(())
    }
    fn cloud_doc(&self, id: DocumentId) -> Option<&Document> {
        self.doc.as_ref().filter(|d| d.id == id).or_else(|| {
            self.background_tabs
                .iter()
                .find(|t| t.doc.id == id)
                .map(|t| &t.doc)
        })
    }
    fn cloud_doc_mut(&mut self, id: DocumentId) -> Option<&mut Document> {
        if self.doc.as_ref().is_some_and(|d| d.id == id) {
            self.doc.as_mut()
        } else {
            self.background_tabs
                .iter_mut()
                .find(|t| t.doc.id == id)
                .map(|t| &mut t.doc)
        }
    }
    fn cloud_install(&mut self, asset: Asset, data: Vec<u8>, cx: &mut Context<Self>) -> Result<()> {
        let mut doc = self
            .registry
            .codecs()
            .find(|c| c.probe(&data))
            .ok_or_else(|| anyhow!("Unsupported remote file format"))?
            .import(&data)?;
        doc.title = asset.name.clone();
        doc.path = None;
        #[allow(unused_mut)]
        let mut shared = remote::document::SharedDocument::unseeded(&doc)?;
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(bytes) = std::fs::read(self.cloud_recovery_path(&asset.id)) {
            match shared.restore(&bytes, &doc) {
                Ok(mut recovered) => {
                    recovered.id = doc.id;
                    doc = recovered;
                }
                Err(e) => self.cloud_error(format!("Could not restore cloud edits: {e}")),
            }
        }
        let id = doc.id;
        let generation = u64::from(doc.dirty);
        self.open_in_tab(doc, true);
        self.cloud_set_visible(false);
        self.cloud.docs.insert(
            id,
            RemoteDocument {
                asset,
                shared,
                joined: false,
                detached: false,
                vector: vec![0],
                sending: false,
                changed: true,
                generation,
                saved: 0,
                render: false,
            },
        );
        self.cloud_join(id);
        cx.notify();
        Ok(())
    }
    pub(crate) fn cloud_open(&mut self, asset: Asset, cx: &mut Context<Self>) {
        if let Some(id) = self
            .cloud
            .docs
            .iter()
            .find(|(_, d)| d.asset.id == asset.id)
            .map(|(id, _)| *id)
        {
            if self.cloud.reopen_document(id) {
                self.cloud_join(id);
            }
            if self.doc.as_ref().is_some_and(|d| d.id == id) {
                self.cloud_set_visible(false);
            } else if let Some(i) = self.background_tabs.iter().position(|t| t.doc.id == id) {
                let index = if i >= self.active_tab { i + 1 } else { i };
                self.select_tab(index, cx);
                self.cloud_set_visible(false);
            }
            cx.notify();
            return;
        }
        let Some(c) = &self.cloud.client else {
            return;
        };
        let handle = c.handle.clone();
        let sender = self.cloud.sender.clone();
        let epoch = self.cloud.epoch;
        self.cloud.message = format!("Opening {}…", asset.name);
        remote::runtime::spawn(async move {
            let result = handle.download_asset_async(&asset.id, None, None).await;
            let job = match result {
                Ok(download) => Job::Opened {
                    epoch,
                    asset,
                    download,
                },
                Err(e) => Job::Error {
                    epoch,
                    error: e.to_string(),
                },
            };
            let _ = sender.send(job);
        });
        cx.notify();
    }
    fn cloud_join(&mut self, id: DocumentId) {
        if !self.cloud.connected || !self.cloud.capabilities_ready {
            return;
        }
        if self
            .cloud
            .capabilities
            .as_ref()
            .is_some_and(|c| !c.supports_image_model())
        {
            self.cloud_error("This provider does not support collaborative image editing");
            return;
        }
        let Some(d) = self.cloud.docs.get_mut(&id) else {
            return;
        };
        if d.detached {
            return;
        }
        if self
            .cloud
            .pending
            .values()
            .any(|p| matches!(p,Pending::Join(doc) if *doc==id))
        {
            return;
        }
        d.joined = false;
        d.sending = false;
        if let Some(c) = &self.cloud.client {
            let req = c.handle.call(
                "document.join",
                map([
                    ("document_id", d.asset.id.clone().into()),
                    ("state_vector", Value::Binary(d.shared.state_vector())),
                ]),
            );
            self.cloud.pending.insert(req, Pending::Join(id));
        }
    }
    pub(crate) fn cloud_undo(&mut self, redo: bool, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.doc.as_ref().map(|d| d.id) else {
            return false;
        };
        self.cloud_capture_edit();
        let Some(d) = self.cloud.docs.get_mut(&id) else {
            return false;
        };
        if !d.joined {
            self.cloud_error("Connect before undoing cloud edits");
            return true;
        }
        if d.shared.undo(redo) {
            d.render = true;
            d.changed = true;
            d.generation += 1;
            if let Err(e) = self.cloud_render_document(id) {
                self.cloud_error(e.to_string());
            }
            self.cloud_send_document(id);
            cx.notify();
        }
        true
    }
    pub(crate) fn cloud_capture_edit(&mut self) {
        let Some(doc) = &self.doc else {
            return;
        };
        let id = doc.id;
        let Some(d) = self.cloud.docs.get_mut(&id) else {
            return;
        };
        if d.shared.revision == doc.revision {
            return;
        }
        match d.shared.local_changes(doc) {
            Ok(Some(_)) => {
                d.changed = true;
                d.generation += 1;
            }
            Ok(None) => {}
            Err(e) => {
                self.cloud_error(format!("Cloud edit is not saved: {e}"));
            }
        }
    }
    fn cloud_send_document(&mut self, id: DocumentId) {
        let Some(d) = self.cloud.docs.get_mut(&id) else {
            return;
        };
        if d.detached || !d.joined || d.sending || !d.changed {
            return;
        }
        if let Some(capabilities) = &self.cloud.capabilities {
            if let Err(error) = capabilities.check_document(d.shared.full_state().len()) {
                d.joined = false;
                self.cloud_error(error.to_string());
                return;
            }
        }
        let update = match d.shared.diff(&d.vector) {
            Ok(b) => b,
            Err(e) => {
                d.joined = false;
                self.cloud_error(e.to_string());
                return;
            }
        };
        if let Some(c) = &self.cloud.client {
            let vector = d.shared.state_vector();
            let generation = d.generation;
            let req = c.handle.call(
                "document.update",
                map([
                    ("document_id", d.asset.id.clone().into()),
                    ("update", Value::Binary(update)),
                ]),
            );
            d.sending = true;
            d.changed = false;
            self.cloud.pending.insert(
                req,
                Pending::Update {
                    doc: id,
                    vector,
                    generation,
                },
            );
        }
    }
    fn cloud_render_document(&mut self, id: DocumentId) -> Result<()> {
        let d = self
            .cloud
            .docs
            .get_mut(&id)
            .ok_or_else(|| anyhow!("No cloud document"))?;
        let mut next = d.shared.render()?;
        next.id = id;
        next.dirty = d.generation > d.saved;
        d.render = false;
        if let Some(doc) = self.cloud_doc_mut(id) {
            next.revision = doc.revision + 1;
            next.active_layer = doc
                .active_layer
                .filter(|id| next.tree.find(*id).is_some())
                .or(next.active_layer);
            next.selection = std::mem::take(&mut doc.selection);
            next.last_selection = doc.last_selection.take();
            next.saved_selections = std::mem::take(&mut doc.saved_selections);
            next.history_source = std::mem::take(&mut doc.history_source);
            next.selected = doc
                .selected
                .iter()
                .copied()
                .filter(|id| next.tree.find(*id).is_some())
                .collect();
            next.active_path = doc.active_path.filter(|i| *i < next.paths.len());
            std::mem::swap(&mut next.history, &mut doc.history);
            *doc = next;
        }
        if let Some(doc) = self.cloud_doc(id) {
            let r = doc.revision;
            if let Some(d) = self.cloud.docs.get_mut(&id) {
                d.shared.revision = r;
            }
        }
        if self.doc.as_ref().is_some_and(|d| d.id == id) {
            self.reset_per_document_caches();
            self.refresh_layer_styles();
        }
        Ok(())
    }
    pub(crate) fn cloud_save(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(id) = self.doc.as_ref().map(|d| d.id) else {
            return false;
        };
        if !self.cloud.docs.contains_key(&id) {
            return false;
        }
        if self
            .cloud
            .capabilities
            .as_ref()
            .is_some_and(|c| !c.supports_image_model())
        {
            self.cloud_error("This provider does not support collaborative image editing; save a local copy instead");
            cx.notify();
            return true;
        }
        self.cloud_capture_edit();
        let remote = &self.cloud.docs[&id];
        if remote.detached {
            self.cloud_error("Cloud sync has stopped for this document. Reopen it from the cloud gallery to reconnect; edits remain local.");
            cx.notify();
            return true;
        }
        if remote.joined && !remote.sending && !remote.changed && remote.saved == remote.generation
        {
            if let Some(doc) = self.cloud_doc_mut(id) {
                doc.mark_saved();
            }
            self.cloud_finish_save(id, cx);
            self.status = "Saved to Schist Cloud".into();
            cx.notify();
            return true;
        }
        if !self.cloud.docs[&id].joined {
            self.cloud_join(id);
        }
        self.cloud_send_document(id);
        self.status = if self.cloud.connected {
            "Saving to Schist Cloud…"
        } else {
            "Offline — cloud edits will sync after reconnecting"
        }
        .into();
        cx.notify();
        true
    }
    fn cloud_finish_save(&mut self, id: DocumentId, cx: &mut Context<Self>) {
        if self.close_after_save != Some(id) {
            return;
        }
        self.close_after_save = None;
        let index = if self.doc.as_ref().is_some_and(|doc| doc.id == id) {
            Some(self.active_tab)
        } else {
            self.background_tabs
                .iter()
                .position(|tab| tab.doc.id == id)
                .map(|i| if i >= self.active_tab { i + 1 } else { i })
        };
        if let Some(index) = index {
            self.close_tab(index, cx);
            self.resume_quit(cx);
        }
    }
    pub(crate) fn cloud_close_document(&mut self, id: DocumentId) {
        self.cloud
            .pending
            .retain(|_, pending| !pending.belongs_to(id));
        if let Some(d) = self.cloud.docs.remove(&id) {
            #[cfg(not(target_arch = "wasm32"))]
            let _ = self
                .cloud
                .recovery
                .send(RecoveryTask::Remove(self.cloud_recovery_path(&d.asset.id)));
            if let Some(c) = &self.cloud.client {
                c.handle
                    .call("document.leave", map([("document_id", d.asset.id.into())]));
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn cloud_recovery_path(&self, asset: &str) -> PathBuf {
        use sha2::Digest;
        let domain = self
            .cloud
            .account
            .as_ref()
            .map(|a| a.domain.as_str())
            .unwrap_or("");
        state_dir().join(format!(
            "{:x}.msgpack",
            sha2::Sha256::digest(format!("{domain}\n{asset}"))
        ))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cloud_checkpoint(&mut self) {
        self.cloud_capture_edit();
        let snapshots = self
            .cloud
            .docs
            .values()
            .map(|d| {
                Ok((
                    self.cloud_recovery_path(&d.asset.id),
                    d.shared.checkpoint()?,
                ))
            })
            .collect::<Result<Vec<_>>>();
        match snapshots {
            Ok(files) => {
                let _ = self.cloud.recovery.send(RecoveryTask::Write {
                    epoch: self.cloud.epoch,
                    files,
                });
            }
            Err(e) => self.cloud_error(format!("Cloud recovery failed: {e}")),
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn cloud_checkpoint(&mut self) {
        self.cloud_capture_edit();
    }
    /// Create or update a cloud bucket from the shared bucket dialog:
    /// the name, and a rule made of the search text and the drawn area.
    /// `form_target` names the bucket being edited (none for a new
    /// one); `form_scope` is what the rule searches. Editing keeps any
    /// other filters the bucket's rule already had.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn cloud_save_bucket(
        &mut self,
        name: String,
        text: Option<String>,
        bounds: Option<remote::Bounds>,
    ) {
        let name = match name.trim() {
            "" => format!("Bucket {}", self.cloud.buckets.len() + 1),
            typed => typed.to_string(),
        };
        let target = self.cloud.form_target.take();
        let mut filters = target
            .as_ref()
            .and_then(|(id, _)| self.cloud.buckets.iter().find(|b| &b.id == id))
            .and_then(|b| b.rule.as_ref())
            .map(|r| r.filters.clone())
            .unwrap_or_default();
        filters.bounds = bounds;
        let rule = if text.is_none() && filters == Filters::default() {
            Value::Nil
        } else {
            value(remote::Rule {
                scope: self.cloud.form_scope.clone(),
                text: text.unwrap_or_default(),
                filters,
            })
        };
        let mut params = vec![("name", name.into()), ("rule", rule)];
        let method = match target {
            Some((id, revision)) => {
                params.extend([("id", id.into()), ("revision", revision.into())]);
                "bucket.update"
            }
            None => "bucket.create",
        };
        self.cloud_mutate(method, params);
    }
    /// "Select all" on a bucket row: show the bucket and select its
    /// page — now if it is already on screen, on arrival otherwise.
    pub(crate) fn cloud_select_all_bucket(&mut self, id: String, cx: &mut Context<Self>) {
        let showing = self.cloud.show
            && self.cloud.loaded
            && matches!(&self.cloud.query.scope, Scope::Bucket { id: on } if on == &id);
        if showing {
            self.cloud.selected = self.cloud_flat_order();
            self.cloud.select_anchor = self.cloud.selected.first().cloned();
            cx.notify();
            return;
        }
        self.cloud_browse(Scope::Bucket { id }, cx);
        self.cloud.select_all_pending = true;
    }
    /// Drop every hand-added member; a smart rule's matches stay.
    pub(crate) fn cloud_clear_bucket(&mut self, bucket: &Bucket) {
        self.cloud_mutate(
            "bucket.clear",
            vec![
                ("id", bucket.id.clone().into()),
                ("revision", bucket.revision.into()),
            ],
        );
    }
    /// Ask which cloud folder a bucket's photos should be filed into.
    pub(crate) fn cloud_move_bucket(&mut self, bucket: &Bucket, cx: &mut Context<Self>) {
        self.cloud.form_target = Some((bucket.id.clone(), bucket.revision));
        self.open_modal(
            Modal::Cloud {
                kind: "move-items",
                fields: vec![("cloud-folder", "Folder".into(), String::new())],
            },
            cx,
        );
    }
    /// Every photo a bucket holds — hand-added and rule-matched —
    /// walked page by page through the provider's query request.
    #[cfg(not(target_arch = "wasm32"))]
    fn cloud_client_for_bucket(
        &mut self,
    ) -> Option<(remote::Handle, Option<remote::Capabilities>)> {
        let Some(c) = &self.cloud.client else {
            self.cloud_error("Sign in first");
            return None;
        };
        if !self.cloud.connected {
            self.cloud_error("Wait for the cloud connection");
            return None;
        }
        Some((c.handle.clone(), self.cloud.capabilities.clone()))
    }
    /// Keep the world map's located assets current for the scope and
    /// search on show: one fetch per change, the whole scope rather than
    /// the page, only photos with a valid fix. Called from the map's
    /// render.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cloud_map_refresh(&mut self) {
        if !self.cloud.show || !self.cloud.connected || self.cloud.map_loading {
            return;
        }
        let mut query = self.cloud.query.clone();
        query.offset = 0;
        query.limit = 500;
        query.sort = "captured_desc".into();
        if query.filters.bounds.is_none() {
            // The provider's bounds filter is also its "has a fix" test.
            query.filters.bounds = Some(remote::Bounds {
                south: -90.0,
                north: 90.0,
                west: -180.0,
                east: 180.0,
            });
        }
        let key = (query.clone(), self.cloud.changes);
        if self.cloud.map_key.as_ref() == Some(&key) {
            return;
        }
        let Some(c) = &self.cloud.client else {
            return;
        };
        let handle = c.handle.clone();
        let sender = self.cloud.sender.clone();
        let epoch = self.cloud.epoch;
        self.cloud.map_loading = true;
        remote::runtime::spawn(async move {
            let result = query_assets(&handle, query, MAP_ASSET_CAP)
                .await
                .map_err(|e| e.to_string());
            let _ = sender.send(Job::MapAssets { epoch, key, result });
        });
    }
    /// Right-click ▸ Download folder…: every photo in a cloud folder
    /// (or the whole library) into a folder of the user's choosing, the
    /// cloud's sub-folders recreated beneath it. Edited photos come as
    /// the provider's default export, the rest as their originals.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cloud_download_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        let Some((handle, capabilities)) = self.cloud_client_for_bucket() else {
            cx.notify();
            return;
        };
        let prompt = self.prompt_for_paths(
            gpui::PathPromptOptions {
                files: false,
                directories: true,
                multiple: false,
                prompt: Some("Download Here".into()),
            },
            cx,
        );
        let sender = self.cloud.sender.clone();
        let epoch = self.cloud.epoch;
        let folders = self.cloud.folders.clone();
        let root = match &scope {
            Scope::Folder { id, .. } => Some(id.clone()),
            _ => None,
        };
        cx.spawn(async move |_this, _cx| {
            let Ok(Ok(Some(mut dirs))) = prompt.await else {
                return;
            };
            let Some(dest) = dirs.pop() else { return };
            remote::runtime::spawn(async move {
                let result: Result<usize> = (async {
                    let assets = scope_assets(&handle, &scope).await?;
                    let total = assets.len();
                    let mut names: HashSet<PathBuf> = HashSet::new();
                    for (done, asset) in assets.into_iter().enumerate() {
                        let _ = sender.send(Job::Done {
                            epoch,
                            message: format!("Downloading {} of {total}\u{2026}", done + 1),
                        });
                        let format = if asset.edited {
                            capabilities
                                .as_ref()
                                .map(|c| c.default_edited_export.clone())
                                .filter(|f| {
                                    capabilities.as_ref().is_some_and(|c| c.supports_export(f))
                                })
                        } else {
                            None
                        };
                        let download = handle
                            .download_asset_async(
                                &asset.id,
                                format.as_deref(),
                                capabilities.as_ref(),
                            )
                            .await?;
                        let mut dir = dest.clone();
                        for name in
                            folder_path_below(&folders, root.as_deref(), asset.folder_id.as_deref())
                        {
                            dir.push(safe_component(&name));
                        }
                        std::fs::create_dir_all(&dir)?;
                        let name = download.suggested_name(&asset.name);
                        let mut path = dir.join(safe_component(&name));
                        if !names.insert(path.clone()) || path.exists() {
                            path = dir.join(format!("{}-{}", done + 1, safe_component(&name)));
                            names.insert(path.clone());
                        }
                        std::fs::write(&path, download.bytes)?;
                    }
                    Ok(total)
                })
                .await;
                let job = match result {
                    Ok(n) => Job::Done {
                        epoch,
                        message: format!(
                            "Downloaded {n} photos to {}",
                            crate::ui::shown_path(&dest)
                        ),
                    },
                    Err(e) => Job::Error {
                        epoch,
                        error: format!("Download failed (finished files remain): {e}"),
                    },
                };
                let _ = sender.send(job);
            });
        })
        .detach();
        self.cloud.message = "Gathering the folder\u{2026}".into();
        cx.notify();
    }
    /// Right-click ▸ Save all as ZIP…: one archive of the bucket's
    /// photos — edited ones as the provider's default export, the rest
    /// as their originals — built straight from the downloads.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cloud_zip_bucket(&mut self, bucket: Bucket, cx: &mut Context<Self>) {
        let Some((handle, capabilities)) = self.cloud_client_for_bucket() else {
            cx.notify();
            return;
        };
        let suggested = format!("{}.zip", bucket.name.to_lowercase().replace(' ', "-"));
        let directory = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let prompt = self.prompt_for_new_path(&directory, Some(&suggested), cx);
        let sender = self.cloud.sender.clone();
        let epoch = self.cloud.epoch;
        cx.spawn(async move |_this, _cx| {
            let Ok(Ok(Some(out))) = prompt.await else {
                return;
            };
            remote::runtime::spawn(async move {
                let progress = |message: String| {
                    let _ = sender.send(Job::Done { epoch, message });
                };
                let result: Result<usize> = (async {
                    let scope = Scope::Bucket {
                        id: bucket.id.clone(),
                    };
                    let assets = scope_assets(&handle, &scope).await?;
                    let total = assets.len();
                    let mut writer = super::library_ops::ZipWriter::create(&out)?;
                    let mut names = HashSet::new();
                    for (done, asset) in assets.into_iter().enumerate() {
                        progress(format!("Zipping {} of {total}\u{2026}", done + 1));
                        let format = if asset.edited {
                            capabilities
                                .as_ref()
                                .map(|c| c.default_edited_export.clone())
                                .filter(|f| {
                                    capabilities.as_ref().is_some_and(|c| c.supports_export(f))
                                })
                        } else {
                            None
                        };
                        let download = handle
                            .download_asset_async(
                                &asset.id,
                                format.as_deref(),
                                capabilities.as_ref(),
                            )
                            .await?;
                        let mut name = download.suggested_name(&asset.name);
                        if !names.insert(name.clone()) {
                            name = format!("{}-{name}", done + 1);
                            names.insert(name.clone());
                        }
                        writer.add(&name, &download.bytes)?;
                    }
                    writer.finish()?;
                    Ok(total)
                })
                .await;
                let job = match result {
                    Ok(n) => Job::Done {
                        epoch,
                        message: format!("Saved {n} photos to {}", crate::ui::shown_path(&out)),
                    },
                    Err(e) => Job::Error {
                        epoch,
                        error: format!("ZIP failed: {e}"),
                    },
                };
                let _ = sender.send(job);
            });
        })
        .detach();
        self.cloud.message = "Gathering the bucket\u{2026}".into();
        cx.notify();
    }
    /// Right-click ▸ Process all…: the bucket's originals land in a
    /// scratch folder, then the batch dialog runs over them; results
    /// save to a folder of the user's choosing.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn cloud_process_bucket(&mut self, bucket: Bucket, cx: &mut Context<Self>) {
        let Some((handle, capabilities)) = self.cloud_client_for_bucket() else {
            cx.notify();
            return;
        };
        let sender = self.cloud.sender.clone();
        let epoch = self.cloud.epoch;
        let dir = batch_dir().join(remote::Uuid::new_v4().to_string());
        remote::runtime::spawn(async move {
            let result: Result<Vec<PathBuf>> = (async {
                let scope = Scope::Bucket {
                    id: bucket.id.clone(),
                };
                let assets = scope_assets(&handle, &scope).await?;
                anyhow::ensure!(!assets.is_empty(), "This bucket is empty");
                std::fs::create_dir_all(&dir)?;
                let total = assets.len();
                let mut paths = Vec::with_capacity(total);
                for (done, asset) in assets.into_iter().enumerate() {
                    let _ = sender.send(Job::Done {
                        epoch,
                        message: format!("Fetching {} of {total}\u{2026}", done + 1),
                    });
                    let download = handle
                        .download_asset_async(&asset.id, None, capabilities.as_ref())
                        .await?;
                    let mut path = dir.join(download.suggested_name(&asset.name));
                    if path.exists() {
                        path = dir.join(format!(
                            "{}-{}",
                            done + 1,
                            download.suggested_name(&asset.name)
                        ));
                    }
                    std::fs::write(&path, download.bytes)?;
                    paths.push(path);
                }
                Ok(paths)
            })
            .await;
            let job = match result {
                Ok(paths) => Job::Batch { epoch, paths },
                Err(e) => Job::Error {
                    epoch,
                    error: format!("Could not fetch the bucket: {e}"),
                },
            };
            let _ = sender.send(job);
        });
        self.cloud.message = "Gathering the bucket\u{2026}".into();
        cx.notify();
    }
    pub(crate) fn cloud_mutate(&mut self, method: &str, fields: Vec<(&'static str, Value)>) {
        if let Some(c) = &self.cloud.client {
            let mut fields = fields;
            fields.push(("mutation_id", remote::Uuid::new_v4().to_string().into()));
            let id = c.handle.call(method, map(fields));
            self.cloud.pending.insert(id, Pending::Mutation);
        }
    }
    pub(crate) fn cloud_drop_remote(&mut self, bucket: String, items: Vec<Value>) {
        self.cloud_mutate(
            "bucket.add",
            vec![("id", bucket.into()), ("items", Value::Array(items))],
        );
    }
    /// The strip's Upload buttons: into whatever is on show.
    pub(crate) fn cloud_pick_upload(&mut self, directory: bool, cx: &mut Context<Self>) {
        let (bucket, folder) = match &self.cloud.query.scope {
            Scope::Bucket { id } => (Some(id.clone()), None),
            Scope::Folder { id, .. } => (None, Some(id.clone())),
            _ => (None, None),
        };
        self.cloud_pick_upload_to(bucket, folder, directory, cx);
    }
    /// Pick files or a folder and upload them into a bucket or folder.
    pub(crate) fn cloud_pick_upload_to(
        &mut self,
        bucket: Option<String>,
        folder: Option<String>,
        directory: bool,
        cx: &mut Context<Self>,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        let prompt = {
            let picker = self.prompt_for_paths(
                gpui::PathPromptOptions {
                    files: !directory,
                    directories: directory,
                    multiple: true,
                    prompt: Some("Upload to Schist Cloud".into()),
                },
                cx,
            );
            async move { picker.await? }
        };
        #[cfg(target_arch = "wasm32")]
        let prompt = crate::web::pick_cloud_files(directory);
        cx.spawn(async move |this, cx| {
            let result = prompt.await;
            let _ = this.update(cx, |ws, cx| {
                match result {
                    Ok(Some(paths)) => ws.cloud_drop_local(bucket, folder, paths, cx),
                    Ok(None) => {}
                    Err(error) => ws.cloud_error(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn cloud_drop_local(
        &mut self,
        bucket: Option<String>,
        folder: Option<String>,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(c) = &self.cloud.client else {
            return;
        };
        let handle = c.handle.clone();
        let sender = self.cloud.sender.clone();
        let epoch = self.cloud.epoch;
        self.cloud.message = "Uploading files…".into();
        self.cloud.progress = Some((0, 0, "Looking through the files…".into()));
        remote::runtime::spawn(async move {
            let progress = |done: u64, total: u64, label: String| {
                let _ = sender.send(Job::Progress {
                    epoch,
                    done,
                    total,
                    label,
                });
            };
            let result: Result<UploadSummary> = (async {
                let mut files = Vec::new();
                for path in paths {
                    if path.is_dir() {
                        enumerate_files(&path, &path, &mut files)?;
                    } else {
                        #[cfg(not(target_arch = "wasm32"))]
                        let relative = None;
                        #[cfg(target_arch = "wasm32")]
                        let relative = crate::web::cloud_relative_path(&path);
                        files.push((path, relative));
                    }
                }
                progress(0, files.len() as u64, "Checking cloud storage…".into());
                let mut selection_bytes = 0u64;
                let mut candidates = Vec::new();
                for (path, relative) in &files {
                    #[cfg(not(target_arch = "wasm32"))]
                    let size = std::fs::metadata(path)
                        .map(|metadata| metadata.len())
                        .unwrap_or(0);
                    #[cfg(target_arch = "wasm32")]
                    let size = crate::web::read_file(path)
                        .map(|bytes| bytes.len() as u64)
                        .unwrap_or(0);
                    if remote::validate_upload_size(size).is_err() {
                        continue;
                    }
                    selection_bytes = selection_bytes
                        .checked_add(size)
                        .ok_or_else(|| anyhow!("Selection is too large"))?;
                    if size > remote::MAX_SINGLE_UPLOAD_BYTES {
                        candidates.push((path, relative, size));
                    }
                }
                let initial: remote::multipart::UploadCapacity = parse(
                    handle
                        .request_async(
                            "asset.check_upload",
                            map([("bytes", selection_bytes.into())]),
                        )
                        .await?,
                )?;
                if !initial.fits {
                    let mut resumes = Vec::new();
                    for (path, relative, size) in candidates {
                        #[cfg(not(target_arch = "wasm32"))]
                        let resume_key = remote::multipart::resume_key_for_path(
                            path,
                            mime(path),
                            folder.as_deref(),
                            relative.as_deref(),
                        )?;
                        #[cfg(target_arch = "wasm32")]
                        let resume_key = remote::multipart::resume_key_for_bytes(
                            &crate::web::read_file(path)?,
                            &path.file_name().unwrap_or_default().to_string_lossy(),
                            mime(path),
                            folder.as_deref(),
                            relative.as_deref(),
                        );
                        resumes.push(map([
                            ("resume_key", resume_key.into()),
                            ("size", size.into()),
                        ]));
                    }
                    let capacity: remote::multipart::UploadCapacity = parse(
                        handle
                            .request_async(
                                "asset.check_upload",
                                map([
                                    ("bytes", selection_bytes.into()),
                                    ("resumes", Value::Array(resumes)),
                                ]),
                            )
                            .await?,
                    )?;
                    capacity.require_space()?;
                }
                let total = files.len() as u64;
                progress(0, total, format!("Uploading 0 of {total} photos…"));
                // A pipeline: files are read and packed into compressed
                // batches ahead of the network, a few at a time, while
                // one batch at a time goes up. The provider's batch
                // support is learned from the first reply and shared
                // back to the packer, so raw files stop being kept once
                // payloads are known to be enough.
                let support = Arc::new(std::sync::atomic::AtomicU8::new(SUPPORT_UNKNOWN));
                let mut uploader = Uploader {
                    handle: handle.clone(),
                    folder: folder.clone(),
                    support: support.clone(),
                    sender: sender.clone(),
                    epoch,
                    total,
                    done: 0,
                    uploaded: Vec::new(),
                    existing: Vec::new(),
                    skipped: Vec::new(),
                    dedupe: SUPPORT_UNKNOWN,
                };
                let preparer = Preparer::new(files, support);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let (tx, rx) = std::sync::mpsc::sync_channel::<Prepared>(PREPARE_AHEAD);
                    std::thread::spawn(move || {
                        let mut preparer = preparer;
                        while let Some(item) = preparer.next() {
                            if tx.send(item).is_err() {
                                break;
                            }
                        }
                    });
                    while let Ok(item) = rx.recv() {
                        uploader.take(item).await?;
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let mut preparer = preparer;
                    while let Some(item) = preparer.next() {
                        uploader.take(item).await?;
                    }
                }
                if let Some(bucket) = bucket {
                    uploader.report("Adding to the bucket…".into());
                    let members: Vec<String> = uploader
                        .uploaded
                        .iter()
                        .chain(uploader.existing.iter())
                        .cloned()
                        .collect();
                    for chunk in members.chunks(1000) {
                        let mutation = remote::Uuid::new_v4().to_string();
                        uploader
                            .retrying("adding to the bucket", || {
                                let items = chunk
                                    .iter()
                                    .map(|id| {
                                        map([("kind", "asset".into()), ("id", id.clone().into())])
                                    })
                                    .collect();
                                handle.request_async(
                                    "bucket.add",
                                    map([
                                        ("id", bucket.clone().into()),
                                        ("items", Value::Array(items)),
                                        ("mutation_id", mutation.clone().into()),
                                    ]),
                                )
                            })
                            .await?;
                    }
                }
                let Uploader {
                    uploaded,
                    existing,
                    skipped,
                    ..
                } = uploader;
                Ok(UploadSummary {
                    uploaded: uploaded.len(),
                    existing: existing.len(),
                    skipped,
                })
            })
            .await;
            let job = match result {
                Ok(summary) => Job::Done {
                    epoch,
                    message: summary.message(),
                },
                Err(e) => Job::Error {
                    epoch,
                    error: if e.to_string().starts_with("Not enough cloud storage.") {
                        e.to_string()
                    } else {
                        format!("Upload failed (completed files remain in Cloud): {e}")
                    },
                },
            };
            let _ = sender.send(job);
        });
        cx.notify();
    }
    pub(crate) fn cloud_upload_document(&mut self, cx: &mut Context<Self>) {
        if self.cloud.account.is_none() {
            self.cloud_sign_in(cx);
            return;
        }
        let folder = match &self.cloud.query.scope {
            Scope::Folder { id, .. } => id.clone(),
            _ => String::new(),
        };
        self.open_modal(
            Modal::Cloud {
                kind: "upload-document",
                fields: vec![(
                    "cloud-folder",
                    "Folder ID (empty for unfiled)".into(),
                    folder,
                )],
            },
            cx,
        );
    }
    pub(crate) fn cloud_download_selected(&mut self, cx: &mut Context<Self>) {
        if !self.cloud.connected || !self.cloud.capabilities_ready {
            self.cloud_error("Wait for the cloud connection before downloading");
            cx.notify();
            return;
        }
        if self.cloud.selected.len() != 1 {
            self.cloud_error("Select one cloud photo to download");
            cx.notify();
            return;
        }
        self.cloud.download_target = self
            .cloud
            .assets
            .iter()
            .find(|asset| self.cloud.selected.contains(&asset.id))
            .cloned();
        self.open_modal(
            Modal::Cloud {
                kind: "download",
                fields: vec![("cloud-download-format", "Format".into(), String::new())],
            },
            cx,
        );
    }
    fn cloud_start_download(&mut self, format: Option<String>) -> Result<()> {
        let asset = self
            .cloud
            .download_target
            .clone()
            .ok_or_else(|| anyhow!("Select a cloud photo first"))?;
        let handle = self
            .cloud
            .client
            .as_ref()
            .ok_or_else(|| anyhow!("Sign in first"))?
            .handle
            .clone();
        let capabilities = self.cloud.capabilities.clone();
        let epoch = self.cloud.epoch;
        let sender = self.cloud.sender.clone();
        self.cloud.message = format!("Downloading {}…", asset.name);
        remote::runtime::spawn(async move {
            let job = match handle
                .download_asset_async(&asset.id, format.as_deref(), capabilities.as_ref())
                .await
            {
                Ok(download) => Job::Downloaded {
                    epoch,
                    name: download.suggested_name(&asset.name),
                    download,
                },
                Err(error) => Job::Error {
                    epoch,
                    error: format!("Cloud download failed: {error}"),
                },
            };
            let _ = sender.send(job);
        });
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn cloud_save_download(
        &mut self,
        name: String,
        download: DownloadedAsset,
        cx: &mut Context<Self>,
    ) {
        let directory = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let prompt = self.prompt_for_new_path(&directory, Some(&name), cx);
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = prompt.await else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn(async move { std::fs::write(&path, download.bytes).map(|()| path) })
                .await;
            let _ = this.update(cx, |ws, cx| {
                match result {
                    Ok(path) => {
                        ws.cloud.message = format!("Downloaded {}", crate::ui::shown_path(&path));
                        ws.status = ws.cloud.message.clone().into();
                    }
                    Err(error) => ws.cloud_error(format!("Could not save download: {error}")),
                }
                cx.notify();
            });
        })
        .detach();
    }
    #[cfg(target_arch = "wasm32")]
    fn cloud_save_download(
        &mut self,
        name: String,
        download: DownloadedAsset,
        cx: &mut Context<Self>,
    ) {
        match crate::web::download_bytes(&name, &download.bytes) {
            Ok(()) => self.cloud.message = format!("Downloaded {name}"),
            Err(e) => self.cloud_error(e.to_string()),
        }
        cx.notify();
    }
    fn cloud_upload_current(&mut self, folder: Option<String>) -> Result<()> {
        let doc = self
            .doc
            .as_ref()
            .ok_or_else(|| anyhow!("Open a document first"))?;
        let data = schist_codec_psd::write_psd(doc)?;
        let id = doc.id;
        let name = format!("{}.psd", doc.title.trim_end_matches(".psd"));
        let handle = self
            .cloud
            .client
            .as_ref()
            .ok_or_else(|| anyhow!("Sign in first"))?
            .handle
            .clone();
        let sender = self.cloud.sender.clone();
        let epoch = self.cloud.epoch;
        remote::runtime::spawn(async move {
            let job = match handle
                .upload_async(remote::Upload {
                    name: &name,
                    bytes: &data,
                    mime: "image/vnd.adobe.photoshop",
                    folder: folder.as_deref(),
                    asset: None,
                    relative: None,
                    mutation: &remote::Uuid::new_v4().to_string(),
                })
                .await
            {
                Ok(asset) => Job::Uploaded {
                    epoch,
                    doc: id,
                    asset,
                },
                Err(e) => Job::Error {
                    epoch,
                    error: e.to_string(),
                },
            };
            let _ = sender.send(job);
        });
        Ok(())
    }
    pub(crate) fn cloud_submit(
        &mut self,
        kind: &str,
        fields: Vec<(&'static str, String, String)>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let get = |key: &str| {
            fields
                .iter()
                .find(|(k, _, _)| *k == key)
                .map(|(_, _, v)| v.trim().to_string())
                .unwrap_or_default()
        };
        if super::cloud_people::submit(self, kind, get)? {
            return Ok(());
        }
        match kind {
            "download" => {
                let format = get("cloud-download-format");
                self.cloud_start_download((!format.is_empty()).then_some(format))?;
            }
            "sign-in" => {
                let domain = remote::auth::domain(&get("cloud-domain"))?;
                self.cloud_login(domain, cx);
            }
            "search" => {
                let mut q = self.cloud.query.clone();
                q.text = get("cloud-query");
                self.cloud.search.set_text(q.text.clone());
                q.offset = 0;
                q.filters = parse_filters(&fields)?;
                if let Some(r) = q.filters.min_rating {
                    anyhow::ensure!(r <= 5, "Rating must be 0–5");
                }
                if let Some(c) = &q.filters.content {
                    anyhow::ensure!(
                        ["all", "safe", "flagged"].contains(&c.as_str()),
                        "Invalid content filter"
                    );
                }
                self.cloud.query = q;
                self.cloud_watch_assets(true);
            }
            "catalogue" => {
                self.cloud.catalogue = get("cloud-query");
                self.cloud.folders_offset = 0;
                self.cloud.buckets_offset = 0;
                self.cloud_refresh_catalogue();
            }
            "new-folder" | "new-subfolder" => {
                // A folder made from the sidebar lands in the folder on
                // show; one made from a folder's menu lands inside it.
                let parent = if kind == "new-subfolder" {
                    self.cloud
                        .form_target
                        .take()
                        .map(|(id, _)| Value::from(id))
                        .unwrap_or(Value::Nil)
                } else {
                    match &self.cloud.query.scope {
                        Scope::Folder { id, .. } => id.clone().into(),
                        _ => Value::Nil,
                    }
                };
                self.cloud_mutate(
                    "folder.create",
                    vec![("name", get("cloud-name").into()), ("parent_id", parent)],
                );
            }
            "delete-asset" => {
                let (id, revision) = self
                    .cloud
                    .form_target
                    .clone()
                    .ok_or_else(|| anyhow!("No photo selected"))?;
                self.cloud.selected.retain(|s| s != &id);
                self.cloud_mutate(
                    "asset.delete",
                    vec![("id", id.into()), ("revision", revision.into())],
                );
            }
            "new-bucket" | "edit-bucket" => {
                let filters = parse_filters(&fields)?;
                let text = get("cloud-query");
                let rule = if text.is_empty() && filters == Filters::default() {
                    Value::Nil
                } else {
                    let scope = if kind == "edit-bucket" {
                        self.cloud.form_scope.clone()
                    } else {
                        match &self.cloud.query.scope {
                            Scope::Bucket { .. } => Scope::Library,
                            s => s.clone(),
                        }
                    };
                    value(remote::Rule {
                        scope,
                        text,
                        filters,
                    })
                };
                let mut params = vec![("name", get("cloud-name").into()), ("rule", rule)];
                let method = if kind == "edit-bucket" {
                    let (id, revision) = self
                        .cloud
                        .form_target
                        .clone()
                        .ok_or_else(|| anyhow!("No bucket selected"))?;
                    params.extend([("id", id.into()), ("revision", revision.into())]);
                    "bucket.update"
                } else {
                    "bucket.create"
                };
                self.cloud_mutate(method, params);
            }
            "rename-folder" => {
                let (id, revision) = self
                    .cloud
                    .form_target
                    .clone()
                    .ok_or_else(|| anyhow!("No folder selected"))?;
                self.cloud_mutate(
                    "folder.update",
                    vec![
                        ("id", id.into()),
                        ("revision", revision.into()),
                        ("name", get("cloud-name").into()),
                    ],
                );
            }
            "delete-folder" | "delete-bucket" => {
                let (id, revision) = self
                    .cloud
                    .form_target
                    .clone()
                    .ok_or_else(|| anyhow!("No item selected"))?;
                let mut params = vec![("id", id.into()), ("revision", revision.into())];
                if kind == "delete-folder" && get("cloud-check-contents") == "1" {
                    params.push(("contents", true.into()));
                }
                self.cloud_mutate(
                    if kind == "delete-folder" {
                        "folder.delete"
                    } else {
                        "bucket.delete"
                    },
                    params,
                );
            }
            "upload-document" => {
                let folder = get("cloud-folder");
                self.cloud_upload_current((!folder.is_empty()).then_some(folder))?;
            }
            "upload-folder" => {
                let path = PathBuf::from(get("cloud-path"));
                anyhow::ensure!(path.is_dir(), "That folder is no longer there");
                let folder = get("cloud-folder");
                self.cloud_drop_local(None, (!folder.is_empty()).then_some(folder), vec![path], cx);
            }
            "move-items" => {
                let (bucket, _) = self
                    .cloud
                    .form_target
                    .take()
                    .ok_or_else(|| anyhow!("No bucket selected"))?;
                let folder = get("cloud-folder");
                self.cloud_mutate(
                    "asset.move",
                    vec![
                        (
                            "items",
                            Value::Array(vec![map([
                                ("kind", "bucket".into()),
                                ("id", bucket.into()),
                            ])]),
                        ),
                        (
                            "folder_id",
                            if folder.is_empty() {
                                Value::Nil
                            } else {
                                folder.into()
                            },
                        ),
                    ],
                );
            }
            _ => return Err(anyhow!("Unknown cloud action")),
        }
        Ok(())
    }
}
/// Where a bucket's originals are staged for the batch dialog.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn batch_dir() -> PathBuf {
    state_dir().join("batch")
}
/// A file or folder name the local disk will take: the cloud's names
/// are free text, and a slash in one must not become a path.
#[cfg(not(target_arch = "wasm32"))]
fn safe_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    match cleaned.trim() {
        "" | "." | ".." => "untitled".to_string(),
        s => s.to_string(),
    }
}
/// The folder names from `root` (exclusive; `None` is the library) down
/// to `folder`, for recreating the cloud's tree on disk. A folder off
/// the catalogue page, or a cycle, stops the walk.
#[cfg(not(target_arch = "wasm32"))]
fn folder_path_below(folders: &[Folder], root: Option<&str>, folder: Option<&str>) -> Vec<String> {
    let mut names = Vec::new();
    let mut at = folder.map(str::to_string);
    for _ in 0..32 {
        let Some(id) = at else { break };
        if root == Some(id.as_str()) {
            break;
        }
        let Some(f) = folders.iter().find(|f| f.id == id) else {
            break;
        };
        names.push(f.name.clone());
        at = f.parent_id.clone();
    }
    names.reverse();
    names
}
/// The most located photos the world map plots for one scope.
#[cfg(not(target_arch = "wasm32"))]
const MAP_ASSET_CAP: usize = 5000;
/// Every asset in a scope, page by page through `assets.query`.
#[cfg(not(target_arch = "wasm32"))]
async fn scope_assets(handle: &remote::Handle, scope: &Scope) -> Result<Vec<Asset>> {
    let query = AssetQuery {
        scope: scope.clone(),
        text: String::new(),
        filters: Filters::default(),
        sort: "name".into(),
        offset: 0,
        limit: 500,
    };
    query_assets(handle, query, usize::MAX).await
}
/// Every asset a query matches, page by page through `assets.query`,
/// up to `cap`.
#[cfg(not(target_arch = "wasm32"))]
async fn query_assets(
    handle: &remote::Handle,
    mut query: AssetQuery,
    cap: usize,
) -> Result<Vec<Asset>> {
    let mut all = Vec::new();
    query.limit = 500;
    loop {
        let page: remote::Snapshot = parse(
            handle
                .request_async("assets.query", value(query.clone()))
                .await?,
        )?;
        let got = page.items.len() as u64;
        for item in page.items {
            all.push(parse::<Asset>(item)?);
        }
        query.offset += got;
        if got == 0 || query.offset >= page.total || all.len() >= cap {
            break;
        }
    }
    Ok(all)
}
/// One batch of a drop: at most this many bytes of files, or this many
/// files, per compressed payload — several payloads for a big drop,
/// each small enough to retry on its own.
const BATCH_BYTES: usize = 48 * 1024 * 1024;
const BATCH_FILES: usize = 250;
struct UploadSummary {
    uploaded: usize,
    /// Left out because the library already held them.
    existing: usize,
    skipped: Vec<String>,
}
impl UploadSummary {
    fn message(&self) -> String {
        let mut uploaded = match self.uploaded {
            0 => "Nothing uploaded".to_string(),
            1 => "Uploaded 1 photo".into(),
            n => format!("Uploaded {n} photos"),
        };
        match self.existing {
            0 => {}
            1 => uploaded.push_str("; 1 was already in Schist Cloud"),
            n => uploaded.push_str(&format!("; {n} were already in Schist Cloud")),
        }
        match self.skipped.first() {
            None => uploaded,
            Some(reason) => format!(
                "{uploaded}; skipped {} file{}: {reason}{}",
                self.skipped.len(),
                if self.skipped.len() == 1 { "" } else { "s" },
                if self.skipped.len() > 1 {
                    format!(" (and {} more)", self.skipped.len() - 1)
                } else {
                    String::new()
                },
            ),
        }
    }
}

fn read_cloud_upload(path: &std::path::Path, relative: Option<String>) -> Result<BatchFile> {
    #[cfg(not(target_arch = "wasm32"))]
    let bytes = {
        use std::io::Read as _;
        let metadata = std::fs::metadata(path)?;
        remote::validate_upload_size(metadata.len())?;
        anyhow::ensure!(
            metadata.len() <= remote::MAX_SINGLE_UPLOAD_BYTES,
            "Use chunk uploads for files over 100 MiB"
        );
        // Bound the read too, in case the source grows after checking its size.
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(remote::MAX_SINGLE_UPLOAD_BYTES + 1)
            .read_to_end(&mut bytes)?;
        bytes
    };
    #[cfg(target_arch = "wasm32")]
    let bytes = {
        let bytes = crate::web::read_file(path)?;
        remote::validate_upload_size(bytes.len() as u64)?;
        bytes.to_vec()
    };
    remote::validate_upload_size(bytes.len() as u64)?;
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    Ok(BatchFile {
        source: path.to_path_buf(),
        path: relative.unwrap_or_else(|| name.clone()),
        name,
        mime: mime(path),
        digest: remote::transfer::sha256_hex(&bytes),
        bytes,
    })
}

struct BatchFile {
    /// Where the bytes came from, to read them again for a repack or a
    /// single upload.
    source: PathBuf,
    /// The relative path inside the drop (sub-folders become cloud
    /// folders), or just the name.
    path: String,
    name: String,
    mime: &'static str,
    /// SHA-256, hex: what the provider deduplicates by.
    digest: String,
    bytes: Vec<u8>,
}
/// A batch member without its bytes: enough to ask the provider whether
/// it already has the file, and to read it again if it must go up on
/// its own.
struct BatchEntry {
    source: PathBuf,
    path: String,
    digest: String,
}
impl From<&BatchFile> for BatchEntry {
    fn from(file: &BatchFile) -> Self {
        Self {
            source: file.source.clone(),
            path: file.path.clone(),
            digest: file.digest.clone(),
        }
    }
}
/// Read a batch's members again, for a repack after deduplication or
/// the single-file fallback.
fn reread(entries: &[BatchEntry]) -> Result<Vec<BatchFile>> {
    entries
        .iter()
        .map(|e| read_cloud_upload(&e.source, Some(e.path.clone())))
        .collect()
}
/// The batch payload: magic, a MessagePack manifest, then the files
/// back to back, gzip-compressed. Returns the payload and the size of
/// the files inside it.
fn pack_batch(files: &[BatchFile]) -> Result<(Vec<u8>, u64)> {
    use std::io::Write as _;
    let manifest = remote::protocol::encode(&map([(
        "files",
        Value::Array(
            files
                .iter()
                .map(|f| {
                    map([
                        ("path", f.path.clone().into()),
                        ("mime_type", f.mime.into()),
                        ("size", (f.bytes.len() as u64).into()),
                    ])
                })
                .collect(),
        ),
    )]))?;
    let total: usize = files.iter().map(|f| f.bytes.len()).sum();
    let mut raw = Vec::with_capacity(12 + manifest.len() + total);
    raw.extend_from_slice(b"SCHISTB1");
    raw.extend_from_slice(&(manifest.len() as u32).to_be_bytes());
    raw.extend_from_slice(&manifest);
    for file in files {
        raw.extend_from_slice(&file.bytes);
    }
    // Photos hardly compress; the fast level keeps the CPU out of the
    // way of the network without pretending otherwise.
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&raw)?;
    Ok((encoder.finish()?, total as u64))
}
#[derive(serde::Deserialize)]
struct BatchTicket {
    batch_id: String,
    put_url: String,
}
#[derive(serde::Deserialize)]
struct BatchCommitted {
    assets: Vec<String>,
}
/// How many packed batches wait ahead of the upload: with the one
/// being packed and the one going up, at most four in memory.
#[cfg(not(target_arch = "wasm32"))]
const PREPARE_AHEAD: usize = 2;
/// The provider's batch support, as the uploader learns it and tells
/// the packer: unknown at first, then batches or singles.
const SUPPORT_UNKNOWN: u8 = 0;
const SUPPORT_BATCH: u8 = 1;
const SUPPORT_SINGLES: u8 = 2;
/// What the packer hands the uploader, in drop order.
enum Prepared {
    /// A batch: its members, and the compressed payload unless the
    /// provider is known to take singles only (an oversized single
    /// file also comes without one). The bytes are not kept: a repack
    /// or a single upload reads them from disk again.
    Batch {
        payload: Option<(Vec<u8>, u64)>,
        entries: Vec<BatchEntry>,
    },
    /// Too big for a batch but small enough for one plain upload: it
    /// goes on its own, never packed.
    Single(BatchEntry),
    /// Too big for a batch: the resumable chunked path reads it itself.
    #[cfg(not(target_arch = "wasm32"))]
    Large {
        path: PathBuf,
        relative: Option<String>,
        size: u64,
    },
    /// Left out, with the reason for the summary.
    Skipped(String),
    /// Packing failed; the upload stops here.
    Failed(anyhow::Error),
}
/// Reads and packs the drop's files into batches, ahead of the
/// network. Runs on its own thread on desktop, inline in the browser.
struct Preparer {
    files: std::vec::IntoIter<(PathBuf, Option<String>)>,
    batch: Vec<BatchFile>,
    batch_bytes: usize,
    ready: std::collections::VecDeque<Prepared>,
    support: Arc<std::sync::atomic::AtomicU8>,
}
impl Preparer {
    fn new(
        files: Vec<(PathBuf, Option<String>)>,
        support: Arc<std::sync::atomic::AtomicU8>,
    ) -> Self {
        Self {
            files: files.into_iter(),
            batch: Vec::new(),
            batch_bytes: 0,
            ready: std::collections::VecDeque::new(),
            support,
        }
    }
    /// The batch so far as one item, packed unless the provider is
    /// known to take singles only; the raw files stay unless batches
    /// are known to work.
    fn flush(&mut self) -> Option<Prepared> {
        if self.batch.is_empty() {
            return None;
        }
        let files = std::mem::take(&mut self.batch);
        self.batch_bytes = 0;
        let support = self.support.load(std::sync::atomic::Ordering::Relaxed);
        let payload = if support == SUPPORT_SINGLES {
            None
        } else {
            match pack_batch(&files) {
                Ok(payload) => Some(payload),
                Err(e) => return Some(Prepared::Failed(e)),
            }
        };
        let entries = files.iter().map(BatchEntry::from).collect();
        Some(Prepared::Batch { payload, entries })
    }
    fn next(&mut self) -> Option<Prepared> {
        loop {
            if let Some(item) = self.ready.pop_front() {
                return Some(item);
            }
            let Some((path, relative)) = self.files.next() else {
                return self.flush();
            };
            #[cfg(not(target_arch = "wasm32"))]
            if let Ok(metadata) = std::fs::metadata(&path) {
                if metadata.len() > remote::MAX_SINGLE_UPLOAD_BYTES
                    && metadata.len() <= remote::MAX_UPLOAD_BYTES
                {
                    // Keep the drop's order: the batch so far goes first.
                    if let Some(batch) = self.flush() {
                        self.ready.push_back(batch);
                    }
                    self.ready.push_back(Prepared::Large {
                        path,
                        relative,
                        size: metadata.len(),
                    });
                    continue;
                }
            }
            let file = match read_cloud_upload(&path, relative) {
                Ok(file) => file,
                Err(error) => {
                    self.ready.push_back(Prepared::Skipped(format!(
                        "{}: {error}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )));
                    continue;
                }
            };
            if file.bytes.len() > BATCH_BYTES {
                if let Some(batch) = self.flush() {
                    self.ready.push_back(batch);
                }
                self.ready
                    .push_back(Prepared::Single(BatchEntry::from(&file)));
                continue;
            }
            if self.batch_bytes + file.bytes.len() > BATCH_BYTES || self.batch.len() >= BATCH_FILES
            {
                if let Some(batch) = self.flush() {
                    self.ready.push_back(batch);
                }
            }
            self.batch_bytes += file.bytes.len();
            self.batch.push(file);
        }
    }
}
/// How long a transfer keeps waiting for the connection to come back:
/// this many rounds of at most half a minute each — about four hours.
const OFFLINE_RETRIES: u32 = 480;
/// Sends prepared items up one at a time and keeps the count.
struct Uploader {
    handle: remote::Handle,
    folder: Option<String>,
    support: Arc<std::sync::atomic::AtomicU8>,
    sender: mpsc::Sender<Job>,
    epoch: u64,
    total: u64,
    done: u64,
    uploaded: Vec<String>,
    /// Assets the library already had, found by digest: left out of the
    /// upload, still added to a bucket drop.
    existing: Vec<String>,
    skipped: Vec<String>,
    /// Whether the provider answers `assets.exists`, learned from the
    /// first reply.
    dedupe: u8,
}
impl Uploader {
    fn report(&self, label: String) {
        let _ = self.sender.send(Job::Progress {
            epoch: self.epoch,
            done: self.done,
            total: self.total,
            label,
        });
    }
    /// Run one network step, and when the connection is what failed,
    /// wait for it to return and run the step again — with the same
    /// mutation IDs, so the provider answers a repeated commit from its
    /// record rather than doing it twice. A refused request (quota, a
    /// bad file, an expired ticket) is an answer and comes straight back.
    async fn retrying<T, Fut>(&self, what: &str, step: impl Fn() -> Fut) -> Result<T>
    where
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut attempt = 0u32;
        loop {
            match step().await {
                Ok(value) => return Ok(value),
                Err(error) if remote::transport::transient(&error) && attempt < OFFLINE_RETRIES => {
                    attempt += 1;
                    self.report(format!(
                        "Connection interrupted while {what} — {} of {} uploaded; waiting…",
                        self.done, self.total
                    ));
                    // The first retry is immediate: the session renews
                    // itself every quarter hour by reconnecting, and that
                    // is over in a moment. Only a repeat failure pauses,
                    // for a gateway that just dropped us and is not ready.
                    if attempt > 1 {
                        let pause = (1u64 << attempt.min(5)).min(30);
                        remote::runtime::sleep(std::time::Duration::from_secs(pause)).await;
                    }
                    if !self.handle.wait_online().await {
                        return Err(error.context("The cloud connection was closed"));
                    }
                    self.report(format!(
                        "Reconnected — resuming {what} ({} of {} uploaded)…",
                        self.done, self.total
                    ));
                }
                Err(error) => return Err(error),
            }
        }
    }
    fn step(&mut self, by: usize) {
        self.done += by as u64;
        let (done, total) = (self.done, self.total);
        self.report(if self.skipped.is_empty() {
            format!("Uploading {done} of {total} photos…")
        } else {
            format!(
                "Uploading {done} of {total} photos ({} skipped)…",
                self.skipped.len()
            )
        });
    }
    async fn take(&mut self, item: Prepared) -> Result<()> {
        match item {
            Prepared::Batch {
                mut payload,
                mut entries,
            } => {
                let count = entries.len();
                // Ask first what the library already holds: those files
                // stay home, and the payload is packed again without
                // them. A provider without the question uploads all.
                if self.dedupe != SUPPORT_SINGLES && !entries.is_empty() {
                    let digests: Vec<String> = entries.iter().map(|e| e.digest.clone()).collect();
                    match self
                        .retrying("checking for duplicates", || {
                            existing_assets(&self.handle, &digests)
                        })
                        .await?
                    {
                        Some(found) => {
                            self.dedupe = SUPPORT_BATCH;
                            let before = entries.len();
                            let existing = &mut self.existing;
                            entries.retain(|e| match found.get(&e.digest) {
                                Some(id) => {
                                    existing.push(id.clone());
                                    false
                                }
                                None => true,
                            });
                            if entries.len() != before {
                                payload = None;
                            }
                        }
                        None => self.dedupe = SUPPORT_SINGLES,
                    }
                }
                if entries.is_empty() {
                    self.step(count);
                    return Ok(());
                }
                let folder = self.folder.clone();
                let mut ids = None;
                if self.support.load(std::sync::atomic::Ordering::Relaxed) != SUPPORT_SINGLES {
                    let (payload, total) = match payload.take() {
                        Some(packed) => packed,
                        None => pack_batch(&reread(&entries)?)?,
                    };
                    {
                        // One set of IDs for the batch: every retry
                        // re-sends the same mutations, so the provider
                        // can answer a repeat from its record.
                        let batch = BatchIds::new();
                        match self
                            .retrying("uploading a batch", || {
                                // The count the provider checks is the
                                // batch as packed — after duplicates
                                // were left out — not the drop's tally.
                                send_batch(
                                    &self.handle,
                                    folder.as_deref(),
                                    &payload,
                                    total,
                                    entries.len(),
                                    &batch,
                                )
                            })
                            .await?
                        {
                            Some(found) => {
                                self.support
                                    .store(SUPPORT_BATCH, std::sync::atomic::Ordering::Relaxed);
                                ids = Some(found);
                            }
                            None => self
                                .support
                                .store(SUPPORT_SINGLES, std::sync::atomic::Ordering::Relaxed),
                        }
                    }
                }
                match ids {
                    Some(ids) => self.uploaded.extend(ids),
                    None => {
                        let files = reread(&entries)?;
                        for file in &files {
                            let mutation = remote::Uuid::new_v4().to_string();
                            let id = self
                                .retrying("uploading a photo", || {
                                    send_single(&self.handle, folder.as_deref(), file, &mutation)
                                })
                                .await?;
                            self.uploaded.push(id);
                        }
                    }
                }
                self.step(count);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Prepared::Large {
                path,
                relative,
                size,
            } => {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.report(format!("Checking {name} for a resumable upload…"));
                let (sender, epoch, done, total) =
                    (self.sender.clone(), self.epoch, self.done, self.total);
                // The chunked upload resumes from the parts already
                // stored, so a retry after an outage picks up where it
                // stopped.
                let asset = self
                    .retrying("uploading a large file", || {
                        let (sender, name) = (sender.clone(), name.clone());
                        self.handle.upload_path_async(
                            &path,
                            mime(&path),
                            self.folder.as_deref(),
                            relative.as_deref(),
                            move |bytes| {
                                let _ = sender.send(Job::Progress {
                                    epoch,
                                    done,
                                    total,
                                    label: format!(
                                        "Uploading {name}: {}% ({} / {} MiB)",
                                        bytes * 100 / size.max(1),
                                        bytes / 1024 / 1024,
                                        size / 1024 / 1024
                                    ),
                                });
                            },
                        )
                    })
                    .await?;
                self.uploaded.push(asset.id);
                self.step(1);
            }
            Prepared::Single(entry) => {
                // Still worth asking whether the library has it.
                if self.dedupe != SUPPORT_SINGLES {
                    let digests = vec![entry.digest.clone()];
                    match self
                        .retrying("checking for duplicates", || {
                            existing_assets(&self.handle, &digests)
                        })
                        .await?
                    {
                        Some(found) => {
                            self.dedupe = SUPPORT_BATCH;
                            if let Some(id) = found.get(&entry.digest) {
                                self.existing.push(id.clone());
                                self.step(1);
                                return Ok(());
                            }
                        }
                        None => self.dedupe = SUPPORT_SINGLES,
                    }
                }
                let folder = self.folder.clone();
                let file = read_cloud_upload(&entry.source, Some(entry.path.clone()))?;
                let mutation = remote::Uuid::new_v4().to_string();
                let id = self
                    .retrying("uploading a photo", || {
                        send_single(&self.handle, folder.as_deref(), &file, &mutation)
                    })
                    .await?;
                self.uploaded.push(id);
                self.step(1);
            }
            Prepared::Skipped(reason) => {
                self.skipped.push(reason);
                self.step(1);
            }
            Prepared::Failed(error) => return Err(error),
        }
        Ok(())
    }
}
#[derive(serde::Deserialize)]
struct ExistingAsset {
    sha256: String,
    id: String,
}
#[derive(serde::Deserialize)]
struct ExistingAssets {
    found: Vec<ExistingAsset>,
}
/// Which of these digests the library already holds, as digest → asset
/// ID. `None` when the provider cannot say, so everything uploads.
async fn existing_assets(
    handle: &remote::Handle,
    digests: &[String],
) -> Result<Option<HashMap<String, String>>> {
    let params = map([(
        "sha256",
        Value::Array(digests.iter().map(|d| d.clone().into()).collect()),
    )]);
    match handle.request_async("assets.exists", params).await {
        Ok(reply) => {
            let found: ExistingAssets = parse(reply)?;
            Ok(Some(
                found.found.into_iter().map(|f| (f.sha256, f.id)).collect(),
            ))
        }
        Err(e) if e.to_string().contains("method_not_found") => Ok(None),
        Err(e) => Err(e),
    }
}
/// The mutation IDs one batch uses, fixed for its lifetime so retries
/// repeat rather than duplicate.
struct BatchIds {
    prepare: String,
    commit: String,
}
impl BatchIds {
    fn new() -> Self {
        Self {
            prepare: remote::Uuid::new_v4().to_string(),
            commit: remote::Uuid::new_v4().to_string(),
        }
    }
}
/// Upload one packed batch: prepare, put the payload, commit. `None`
/// when the provider has no batch method, so the caller falls back to
/// singles. Repeating the call after an outage repeats the same
/// mutations: a prepare already answered returns its ticket, a commit
/// already done returns its assets.
async fn send_batch(
    handle: &remote::Handle,
    folder: Option<&str>,
    payload: &[u8],
    total: u64,
    count: usize,
    ids: &BatchIds,
) -> Result<Option<Vec<String>>> {
    let mut fields = vec![
        ("size", (payload.len() as u64).into()),
        ("total", total.into()),
        ("count", (count as u64).into()),
        ("mutation_id", ids.prepare.clone().into()),
    ];
    if let Some(folder) = folder {
        fields.push(("folder_id", folder.into()));
    }
    let ticket: BatchTicket = match handle
        .request_async("asset.prepare_batch", map(fields))
        .await
    {
        Ok(reply) => parse(reply)?,
        Err(e) if e.to_string().contains("method_not_found") => return Ok(None),
        Err(e) => return Err(e),
    };
    remote::auth::upload_async(&ticket.put_url, "application/gzip", payload).await?;
    let committed: BatchCommitted = parse(
        handle
            .request_async(
                "asset.commit_batch",
                map([
                    ("batch_id", ticket.batch_id.into()),
                    ("mutation_id", ids.commit.clone().into()),
                ]),
            )
            .await?,
    )?;
    Ok(Some(committed.assets))
}
/// The one-file path: a provider without batches, or a file too big for
/// one.
async fn send_single(
    handle: &remote::Handle,
    folder: Option<&str>,
    file: &BatchFile,
    mutation: &str,
) -> Result<String> {
    let relative = (file.path != file.name).then_some(file.path.as_str());
    let asset = handle
        .upload_async(remote::Upload {
            name: &file.name,
            bytes: &file.bytes,
            mime: file.mime,
            folder,
            asset: None,
            relative,
            mutation,
        })
        .await?;
    Ok(asset.id)
}
fn mime(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "tif" | "tiff" => "image/tiff",
        "psd" | "psb" => "image/vnd.adobe.photoshop",
        _ => "application/octet-stream",
    }
}
fn enumerate_files(
    root: &std::path::Path,
    path: &std::path::Path,
    out: &mut Vec<(PathBuf, Option<String>)>,
) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            continue;
        }
        // Hidden entries stay home: the gallery's own `.schist` sidecars
        // and version folders, `.DS_Store`, thumbnail caches — none of
        // them are photos anyone meant to upload.
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if ty.is_dir() {
            enumerate_files(root, &entry.path(), out)?;
        } else if ty.is_file() {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((path, Some(relative)));
        }
    }
    Ok(())
}

fn parse_filters(fields: &[(&'static str, String, String)]) -> Result<Filters> {
    let get = |key: &str| {
        fields
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, _, v)| v.trim())
            .unwrap_or("")
    };
    let list = |key: &str| {
        let s = get(key);
        (!s.is_empty()).then(|| {
            s.split(',')
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect()
        })
    };
    let edited = match get("cloud-edited") {
        "any" | "" => None,
        "yes" => Some(true),
        "no" => Some(false),
        _ => return Err(anyhow!("Edited must be any, yes or no")),
    };
    let content = match get("cloud-content") {
        "all" | "" => None,
        "safe" => Some("safe".into()),
        "flagged" => Some("flagged".into()),
        _ => return Err(anyhow!("Content must be all, safe or flagged")),
    };
    let rating = get("cloud-rating");
    let min_rating = if rating.is_empty() {
        None
    } else {
        let r: u8 = rating.parse()?;
        anyhow::ensure!(r <= 5, "Rating must be between 0 and 5");
        Some(r)
    };
    let b = get("cloud-bounds");
    let bounds = if b.is_empty() {
        None
    } else {
        let c = b
            .split(',')
            .map(|s| s.trim().parse::<f64>())
            .collect::<std::result::Result<Vec<_>, _>>()?;
        anyhow::ensure!(
            c.len() == 4
                && c.iter().all(|v| v.is_finite())
                && c[0] >= -90.0
                && c[2] <= 90.0
                && c[0] <= c[2]
                && c[1].abs() <= 180.0
                && c[3].abs() <= 180.0,
            "Enter valid south, west, north, east coordinates"
        );
        Some(remote::Bounds {
            south: c[0],
            west: c[1],
            north: c[2],
            east: c[3],
        })
    };
    let captured_after = remote::parse_date(get("cloud-after"), false)?;
    let captured_before = remote::parse_date(get("cloud-before"), true)?;
    if let (Some(a), Some(b)) = (captured_after, captured_before) {
        anyhow::ensure!(a <= b, "End date must follow start date");
    }
    Ok(Filters {
        person_id: None,
        mime_types: list("cloud-types"),
        tags: list("cloud-tags"),
        edited,
        content,
        min_rating,
        bounds,
        captured_after,
        captured_before,
    })
}

pub(super) fn rgba_to_render_image(
    width: u32,
    height: u32,
    mut rgba: Vec<u8>,
) -> Option<Arc<RenderImage>> {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, rgba)?;
    Some(Arc::new(RenderImage::new(smallvec![image::Frame::new(
        buffer
    )])))
}

#[cfg(test)]
mod cloud_lifecycle_tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn oversized_and_empty_files_do_not_discard_the_valid_batch() {
        use std::io::Read as _;
        let root = std::env::temp_dir().join(format!("schist-upload-{}", remote::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("before.jpg"), b"before").unwrap();
        std::fs::File::create(root.join("large.mov"))
            .unwrap()
            .set_len(remote::MAX_UPLOAD_BYTES + 1)
            .unwrap();
        std::fs::write(root.join("empty.jpg"), b"").unwrap();
        std::fs::write(root.join("after.jpg"), b"after").unwrap();
        let mut batch = Vec::new();
        let mut skipped = Vec::new();
        for name in ["before.jpg", "large.mov", "empty.jpg", "after.jpg"] {
            match read_cloud_upload(&root.join(name), Some(format!("trip/{name}"))) {
                Ok(file) => batch.push(file),
                Err(error) => skipped.push(format!("{name}: {error}")),
            }
        }
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].path, "trip/before.jpg");
        assert_eq!(batch[1].path, "trip/after.jpg");
        let (payload, total) = pack_batch(&batch).unwrap();
        assert_eq!(total, 11);
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(payload.as_slice())
            .read_to_end(&mut raw)
            .unwrap();
        let length = u32::from_be_bytes(raw[8..12].try_into().unwrap()) as usize;
        assert_eq!(&raw[12 + length..], b"beforeafter");
        let message = UploadSummary {
            uploaded: batch.len(),
            existing: 0,
            skipped: skipped.clone(),
        }
        .message();
        assert!(message.contains("Uploaded 2 photos; skipped 2 files"));
        let deduped = UploadSummary {
            uploaded: batch.len(),
            existing: 3,
            skipped,
        }
        .message();
        assert!(deduped.contains("Uploaded 2 photos; 3 were already in Schist Cloud; skipped 2"));
        assert!(message.contains("large.mov"));
        assert!(message.contains("5 GiB"));
        std::fs::remove_dir_all(root).unwrap();
    }
    fn binding(asset: &str) -> RemoteDocument {
        let mut source = Document::new("Original", 1, 1, schist_color::Depth::Eight);
        let mut shared = remote::document::SharedDocument::new(&source).unwrap();
        source.title = "Unsynced local edit".into();
        shared.local_changes(&source).unwrap();
        RemoteDocument {
            asset: Asset {
                faces: Vec::new(),
                moderation: None,
                id: asset.into(),
                folder_id: None,
                name: "Original".into(),
                mime_type: "image/png".into(),
                revision: 1,
                size: 1,
                edited: false,
                tags: vec![],
                rating: 0,
                captured_at: None,
                modified_at: 0,
                thumbnail_url: None,
                place_name: None,
                location: None,
            },
            shared,
            joined: true,
            detached: false,
            vector: vec![0],
            sending: true,
            changed: true,
            generation: 1,
            saved: 0,
            render: true,
        }
    }
    #[test]
    fn terminated_document_keeps_edits_ignores_late_events_and_requires_explicit_reopen() {
        let mut state = CloudState::default();
        let id = DocumentId(41);
        let other = DocumentId(42);
        state.docs.insert(id, binding("revoked"));
        state.docs.insert(other, binding("other"));
        let before = state.docs[&id].shared.full_state();
        state.pending.insert("late-join".into(), Pending::Join(id));
        state.pending.insert(
            "late-update".into(),
            Pending::Update {
                doc: id,
                vector: vec![0],
                generation: 1,
            },
        );
        state
            .pending
            .insert("other-join".into(), Pending::Join(other));
        assert_eq!(state.detach_document("revoked"), Some(id));
        assert!(!state.pending.contains_key("late-join"));
        assert!(!state.pending.contains_key("late-update"));
        assert!(state.pending.contains_key("other-join"));
        // Even a malformed late push is discarded before touching the detached CRDT.
        state.apply_document_update("revoked", &[255]).unwrap();
        state.disconnect();
        assert_eq!(state.joinable_documents(), vec![other]);
        let document = &state.docs[&id];
        assert_eq!(document.shared.full_state(), before);
        assert_eq!((document.generation, document.saved), (1, 0));
        assert!(!document.joined && !document.sending && !document.render);
        assert!(state.reopen_document(id));
        assert!(!state.reopen_document(id));
        assert!(state.joinable_documents().contains(&id));
        assert!(state.apply_document_update("revoked", &[255]).is_err());
    }
}
