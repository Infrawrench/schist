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
use schist_cloud_transfer::{upload_files, Report, UploadSummary};
use schist_core::DocumentId;
#[cfg(not(target_arch = "wasm32"))]
use schist_i18n::tn;
use schist_i18n::{t, tf};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

#[cfg(not(target_arch = "wasm32"))]
use schist_camera_sync::{state_dir, CREDENTIAL_KEY};

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
pub(super) enum Job {
    Thumbnail {
        epoch: u64,
        id: String,
        revision: u64,
        image: Option<Arc<RenderImage>>,
    },
    /// The camera-roll backup's engine and platform bridges.
    #[cfg(not(target_arch = "wasm32"))]
    Sync {
        epoch: u64,
        revision: u64,
        job: super::camera_sync::SyncJob,
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
/// Assets per live query, with another page subscribed near the grid's end.
pub(crate) const PAGE_SIZE: u64 = 200;
/// Concurrent thumbnail fetches, and how many decoded thumbnails stay
/// in memory (~256 KB each) before the cache shrinks to the viewport.
const THUMBNAIL_WORKERS: usize = 8;
const THUMBNAIL_CACHE: usize = 600;

struct AssetPage {
    watch: String,
    offset: u64,
    assets: Vec<Asset>,
    pending: bool,
    error: Option<String>,
}

impl AssetPage {
    fn new(offset: u64) -> Self {
        Self {
            watch: remote::Uuid::new_v4().to_string(),
            offset,
            assets: Vec::new(),
            pending: true,
            error: None,
        }
    }
}
fn local_upload_files(paths: &[PathBuf]) -> Result<Vec<(PathBuf, Option<String>)>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            enumerate_files(path, path, &mut files)?;
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            let relative = None;
            #[cfg(target_arch = "wasm32")]
            let relative = crate::web::cloud_relative_path(path);
            files.push((path.clone(), relative));
        }
    }
    Ok(files)
}

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
    pub(super) face_previews: HashMap<(gpui::ImageId, [u32; 4]), Arc<RenderImage>>,
    thumbnail_jobs: HashSet<(String, u64)>,
    thumbnail_active: usize,
    pub folders: Vec<Folder>,
    pub buckets: Vec<Bucket>,
    pub assets: Vec<Asset>,
    asset_pages: Vec<AssetPage>,
    /// Photos within one viewport of the grid, recorded by cell probes.
    pub(super) grid_wanted: HashSet<String>,
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
    /// Whether the current query has delivered its first batch.
    pub loaded: bool,
    /// A failed watch is unavailable, not an empty library or an ongoing load.
    pub load_error: Option<String>,
    /// "Select all" asked before the first batch arrived: select it on landing.
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
    /// The photo being viewed or named survives changes to the gallery page.
    pub people_target: Option<Asset>,
    pub face_bounds: Bounds<Pixels>,
    pub face_start: Option<(f32, f32)>,
    pub face_draft: Option<remote::FaceRect>,
    pub face_drawing: bool,
    pub buckets_total: u64,
    pub docs: HashMap<DocumentId, RemoteDocument>,
    pending: HashMap<String, Pending>,
    pub epoch: u64,
    jobs: mpsc::Receiver<Job>,
    pub(super) sender: mpsc::Sender<Job>,
    /// The camera-roll backup, while the app is open.
    #[cfg(not(target_arch = "wasm32"))]
    pub sync: super::camera_sync::SyncState,
    pub(super) cancel: Arc<AtomicBool>,
    writes: VecDeque<Option<Account>>,
    #[cfg(not(target_arch = "wasm32"))]
    writing: bool,
    #[cfg(not(target_arch = "wasm32"))]
    recovery: mpsc::Sender<RecoveryTask>,
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
            if crate::feature_enabled("schist-cloud") {
                std::thread::spawn(move || {
                    while let Ok(task) = tasks.recv() {
                        match task {
                            RecoveryTask::Write { epoch, files } => {
                                for (path, bytes) in files {
                                    if let Err(e) = remote::auth::private_write(&path, &bytes) {
                                        let _ = errors.send(Job::Error {
                                            epoch,
                                            error: tf!("cloud.error.recovery_failed", error = e),
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
            }
            recovery
        };
        Self {
            generation: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            sync: Default::default(),
            account: None,
            client: None,
            connected: false,
            capabilities: None,
            capabilities_ready: false,
            download_target: None,
            show: false,
            message: t("cloud.msg.not_signed_in").into(),
            thumbnails: HashMap::new(),
            face_previews: HashMap::new(),
            thumbnail_jobs: HashSet::new(),
            thumbnail_active: 0,
            folders: vec![],
            buckets: vec![],
            assets: vec![],
            asset_pages: Vec::new(),
            grid_wanted: HashSet::new(),
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
            people_target: None,
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
    fn next_asset_offset(&self) -> Option<u64> {
        if !self.loaded
            || self.load_error.is_some()
            || self.asset_pages.iter().any(|page| page.pending)
        {
            return None;
        }
        let page = self.asset_pages.last()?;
        // Use the actual count, since a provider may return smaller batches.
        // An empty batch must never spin on the same offset.
        let next = page.offset + page.assets.len() as u64;
        (!page.assets.is_empty() && next < self.total).then_some(next)
    }

    pub(super) fn is_loading_more(&self) -> bool {
        self.loaded && self.load_error.is_none() && self.asset_pages.iter().any(|page| page.pending)
    }

    fn apply_asset_snapshot(&mut self, id: &str, snapshot: remote::Snapshot) -> Result<bool> {
        let Some(index) = self.asset_pages.iter().position(|page| page.watch == id) else {
            return Ok(false);
        };
        let assets = snapshot
            .items
            .into_iter()
            .map(parse)
            .collect::<Result<_>>()?;
        let page = &mut self.asset_pages[index];
        page.assets = assets;
        page.pending = false;
        page.error = None;
        if index == 0 {
            self.people = snapshot.people;
        }
        self.face_previews.clear();
        self.total = snapshot.total;
        self.loaded |= index == 0;
        self.load_error = self.asset_pages.iter().find_map(|page| page.error.clone());
        self.assets.clear();
        let mut positions = HashMap::new();
        for asset in self.asset_pages.iter().flat_map(|page| &page.assets) {
            // Live page boundaries can briefly overlap while updates arrive.
            // Keep one cell per photo, using its newest revision.
            let position = *positions.entry(&asset.id).or_insert(self.assets.len());
            if position == self.assets.len() {
                self.assets.push(asset.clone());
            } else if asset.revision > self.assets[position].revision {
                self.assets[position] = asset.clone();
            }
        }
        self.refresh_people_target();
        self.changes += 1;
        Ok(true)
    }

    fn fail_asset_page(&mut self, id: &str, error: &str) -> bool {
        let Some(page) = self.asset_pages.iter_mut().find(|page| page.watch == id) else {
            return false;
        };
        page.pending = false;
        page.error = Some(error.into());
        self.load_error = Some(error.into());
        if !self.loaded {
            self.assets.clear();
            self.total = 0;
        }
        true
    }

    pub(super) fn people_asset(&self, id: &str) -> Option<&Asset> {
        self.assets
            .iter()
            .find(|asset| asset.id == id)
            .or_else(|| self.people_target.as_ref().filter(|asset| asset.id == id))
    }

    fn refresh_people_target(&mut self) {
        // A missing photo may have left this filtered page; it does not mean
        // that the photo being viewed was deleted.
        if let Some(target) = &mut self.people_target {
            if let Some(asset) = self.assets.iter().find(|asset| asset.id == target.id) {
                *target = asset.clone();
            }
        }
    }

    /// Resolve older avatar metadata from the page when possible. New providers
    /// include a ticket so the sidebar also works for photos outside that page.
    pub(super) fn avatar_source(
        &self,
        avatar: &remote::PersonAvatar,
    ) -> Option<(u64, Option<String>)> {
        if let Some(revision) = avatar.revision {
            return Some((revision, avatar.thumbnail_url.clone()));
        }
        self.assets
            .iter()
            .chain(&self.map_assets)
            .find(|asset| asset.id == avatar.asset_id)
            .map(|asset| (asset.revision, asset.thumbnail_url.clone()))
    }
    fn wanted_thumbnails(&self) -> Vec<(String, u64, Option<String>)> {
        // Cloud people remain visible beside local albums, even with the cloud
        // grid closed. Keep their source photos in the shared thumbnail cache.
        let portraits = self
            .people
            .iter()
            .flat_map(|people| &people.people)
            .filter_map(|person| {
                let avatar = person.avatar.as_ref()?;
                let (revision, url) = self.avatar_source(avatar)?;
                Some((avatar.asset_id.clone(), revision, url))
            });
        let sources = portraits
            .chain(
                self.assets
                    .iter()
                    .filter(|asset| self.grid_wanted.contains(&asset.id))
                    .chain(
                        self.map_assets
                            .iter()
                            .filter(|a| self.map_wanted.contains(&a.id)),
                    )
                    .filter(|_| self.show)
                    .map(|a| (a.id.clone(), a.revision, a.thumbnail_url.clone())),
            )
            .chain(
                self.people_target
                    .iter()
                    .map(|a| (a.id.clone(), a.revision, a.thumbnail_url.clone())),
            );
        let mut positions = HashMap::new();
        let mut wanted: Vec<(String, u64, Option<String>)> = Vec::new();
        for source in sources {
            let position = *positions.entry(source.0.clone()).or_insert(wanted.len());
            if position == wanted.len() {
                wanted.push(source);
            } else {
                // A map snapshot may still refer to an earlier revision. One
                // source per photo keeps an old response from replacing it.
                let current = &mut wanted[position];
                if source.1 > current.1 || (source.1 == current.1 && current.2.is_none()) {
                    *current = source;
                }
            }
        }
        wanted
    }
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
        if !crate::feature_enabled("schist-cloud") {
            // Retire scheduled mobile backups without changing the saved
            // account or rule, which can be used again when Cloud is enabled.
            #[cfg(not(target_arch = "wasm32"))]
            self.camera_sync_update_background();
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            #[cfg(target_os = "android")]
            let handover = cx.background_executor().spawn(async {
                super::camera_sync_android::hand_over_to_activity();
            });
            let epoch = self.cloud.epoch;
            cx.spawn(async move |this, cx| {
                #[cfg(target_os = "android")]
                handover.await;
                let Ok(read) = this.update(cx, |_, cx| cx.read_credentials(CREDENTIAL_KEY)) else {
                    return;
                };
                let result = read.await;
                let _ = this.update(cx, |ws, cx| {
                    if ws.cloud.epoch != epoch {
                        return;
                    }
                    match result {
                        Ok(Some((_, data))) => match serde_json::from_slice::<Account>(&data) {
                            Ok(account) => ws.cloud_connect(account, cx),
                            Err(e) => {
                                ws.cloud_error(tf!("cloud.error.stored_login_invalid", error = e))
                            }
                        },
                        Ok(None) => {}
                        Err(e) => {
                            ws.cloud_error(tf!("cloud.error.could_not_read_login", error = e))
                        }
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
    pub(super) fn cloud_error(&mut self, error: impl Into<String>) {
        let error = error.into();
        self.status = error.clone().into();
        self.cloud.message = error;
    }
    pub(crate) fn cloud_sign_in(&mut self, cx: &mut Context<Self>) {
        if !crate::feature_enabled("schist-cloud") {
            return;
        }
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
                        t("cloud.dialog.domain").into(),
                        remote::DEFAULT_DOMAIN.into(),
                    )],
                },
                cx,
            );
            self.focus_field("cloud-domain", remote::DEFAULT_DOMAIN);
        }
    }
    fn cloud_login(&mut self, domain: String, cx: &mut Context<Self>) {
        if !crate::feature_enabled("schist-cloud") {
            return;
        }
        self.cloud.epoch += 1;
        let epoch = self.cloud.epoch;
        self.cloud.cancel.store(true, Ordering::Relaxed);
        self.cloud.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cloud.cancel.clone();
        let sender = self.cloud.sender.clone();
        self.cloud.message = t("cloud.status.opening_sign_in").into();
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
                    error: tf!("cloud.error.sign_in_failed", error = e),
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
        if !crate::feature_enabled("schist-cloud") {
            return;
        }
        self.cloud.library_total = None;
        self.cloud.people_target = None;
        self.cloud.client = Some(Client::start(account.clone()));
        self.cloud.account = Some(account.clone());
        #[cfg(target_os = "android")]
        super::camera_sync_android::restore_status(&mut self.view.camera_sync);
        #[cfg(not(target_arch = "wasm32"))]
        self.camera_sync_update_background();
        self.cloud.writes.push_back(Some(account));
        self.cloud.message = t("cloud.status.connecting").into();
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
        #[cfg(not(target_arch = "wasm32"))]
        self.camera_sync_sign_out();
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
        self.cloud.asset_pages.clear();
        self.cloud.grid_wanted.clear();
        self.cloud.people = None;
        self.cloud.people_target = None;
        self.cloud.thumbnails.clear();
        self.cloud.face_previews.clear();
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
        self.cloud.message = t("cloud.status.signed_out").into();
        if let Some(account) = account {
            let sender = self.cloud.sender.clone();
            let epoch = self.cloud.epoch;
            remote::runtime::spawn(async move {
                if let Err(e) = remote::auth::logout_async(&account).await {
                    let _ = sender.send(Job::Error {
                        epoch,
                        error: tf!("cloud.error.server_logout_failed", error = e),
                    });
                }
            });
        }
        cx.notify();
    }
    pub(crate) fn cloud_set_visible(&mut self, visible: bool) {
        if visible && !crate::feature_enabled("schist-cloud") {
            return;
        }
        self.cloud.show = visible;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.library.open = visible;
        }
    }
    pub(crate) fn cloud_browse(&mut self, scope: Scope, cx: &mut Context<Self>) {
        if !crate::feature_enabled("schist-cloud") {
            return;
        }
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
    /// Start the query again from its first page. `keep` leaves the old
    /// results on screen until the replacement arrives.
    pub(crate) fn cloud_watch_assets(&mut self, keep: bool) {
        self.cloud.query.sort = self.cloud_sort();
        self.cloud.query.offset = 0;
        self.cloud.query.limit = PAGE_SIZE;
        self.cloud.loaded = false;
        self.cloud.load_error = None;
        self.cloud.grid.handle.set_offset(point(px(0.0), px(0.0)));
        self.cloud.grid_wanted.clear();
        if !keep {
            self.cloud.assets.clear();
            self.cloud.total = 0;
        }
        for page in self.cloud.asset_pages.drain(..) {
            if let Some(c) = &self.cloud.client {
                c.handle.unwatch(&page.watch);
            }
        }
        self.cloud_watch_asset_page(0);
    }

    fn cloud_watch_asset_page(&mut self, offset: u64) {
        if let Some(c) = &self.cloud.client {
            let page = AssetPage::new(offset);
            let mut query = self.cloud.query.clone();
            query.offset = offset;
            c.handle.watch(
                &page.watch,
                WatchQuery::Assets {
                    query: Box::new(query),
                },
            );
            self.cloud.asset_pages.push(page);
        }
    }

    pub(super) fn cloud_load_more(&mut self, cx: &mut Context<Self>) {
        if !self.cloud.show || !self.cloud.connected || self.cloud.client.is_none() {
            return;
        }
        if let Some(offset) = self.cloud.next_asset_offset() {
            self.cloud_watch_asset_page(offset);
            cx.notify();
        }
    }

    pub(super) fn cloud_retry_assets(&mut self, cx: &mut Context<Self>) {
        let Some(c) = &self.cloud.client else {
            return;
        };
        for page in &mut self.cloud.asset_pages {
            if page.error.take().is_none() {
                continue;
            }
            c.handle.unwatch(&page.watch);
            page.watch = remote::Uuid::new_v4().to_string();
            page.pending = true;
            let mut query = self.cloud.query.clone();
            query.offset = page.offset;
            c.handle.watch(
                &page.watch,
                WatchQuery::Assets {
                    query: Box::new(query),
                },
            );
        }
        self.cloud.load_error = None;
        cx.notify();
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
        if self.cloud.account.is_none() {
            return;
        }
        // A small worker pool bounds the network; decoded thumbnails
        // stay for a few viewports, so scrolling back is instant; an
        // overfull cache falls back to the photos near the viewport.
        let wanted = self.cloud.wanted_thumbnails();
        let visible: HashSet<String> = wanted.iter().map(|(id, _, _)| id.clone()).collect();
        if self.cloud.thumbnails.len() > THUMBNAIL_CACHE {
            self.cloud.thumbnails.retain(|id, _| visible.contains(id));
            self.cloud.face_previews.clear();
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
        #[cfg(not(target_arch = "wasm32"))]
        self.camera_sync_tick(cx);
        let mut changed = false;
        while let Ok(job) = self.cloud.jobs.try_recv() {
            changed = true;
            match job {
                #[cfg(not(target_arch = "wasm32"))]
                Job::Sync {
                    epoch,
                    revision,
                    job,
                } if epoch == self.cloud.epoch && revision == self.cloud.sync.revision => {
                    self.camera_sync_job(job, cx)
                }
                Job::Thumbnail {
                    epoch,
                    id,
                    revision,
                    image,
                } if epoch == self.cloud.epoch => {
                    self.cloud.thumbnail_active = self.cloud.thumbnail_active.saturating_sub(1);
                    // A face's source may have changed revision while its old
                    // thumbnail was downloading. Do not replace the new image.
                    if !self
                        .cloud
                        .wanted_thumbnails()
                        .iter()
                        .any(|(wanted, r, _)| wanted == &id && *r == revision)
                    {
                        self.cloud.thumbnail_jobs.remove(&(id, revision));
                        continue;
                    }
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
                    #[cfg(not(target_arch = "wasm32"))]
                    if self.cloud.account.is_some() {
                        self.camera_sync_sign_out();
                    }
                    self.cloud_connect(account, cx);
                    // A sign-in someone just made, not a stored login
                    // restored at launch: the moment to ask, once, about
                    // the camera roll.
                    #[cfg(not(target_arch = "wasm32"))]
                    if super::camera_sync::offered() && !self.view.camera_sync.asked {
                        self.cloud.sync.prompt_pending = true;
                    }
                }
                Job::Error { epoch, error } if epoch == self.cloud.epoch => {
                    self.cloud.progress = None;
                    if error.starts_with(t("cloud.error.not_enough_storage")) {
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
                        Err(error) => self.cloud_error(tf!("cloud.error.world_map", error = error)),
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
                        self.cloud_error(tf!("cloud.error.open_document", error = e));
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
                        self.status = t("cloud.status.uploaded_screening").into();
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
                #[cfg(target_os = "ios")]
                if result.is_ok() {
                    if let Err(error) =
                        super::camera_sync_ios::background_credentials(ws.view.camera_sync.enabled)
                    {
                        ws.cloud_error(tf!("cloud.sync.keychain_failed", error = error));
                    }
                }
                if let Err(e) = result {
                    ws.cloud_error(tf!("cloud.error.could_not_persist_login", error = e));
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
                self.cloud.load_error = self
                    .cloud
                    .asset_pages
                    .iter()
                    .find_map(|page| page.error.clone());
                self.cloud.capabilities = None;
                self.cloud.capabilities_ready = false;
                self.cloud.message = t("cloud.status.connected").into();
                if let Some(client) = &self.cloud.client {
                    let id = client.handle.call("workspace.capabilities", map([]));
                    self.cloud.pending.insert(id, Pending::Capabilities);
                }
            }
            Event::AccountUnavailable => {
                #[cfg(not(target_arch = "wasm32"))]
                self.camera_sync_sign_out();
                self.cloud.disconnect();
                self.cloud.epoch += 1;
                self.cloud.cancel.store(true, Ordering::Relaxed);
                self.cloud.account = None;
                self.cloud.client = None;
                self.cloud.writes.push_back(None);
                self.cloud.assets.clear();
                self.cloud.asset_pages.clear();
                self.cloud.grid_wanted.clear();
                self.cloud.folders.clear();
                self.cloud.buckets.clear();
                self.cloud.thumbnails.clear();
                self.cloud.face_previews.clear();
                self.cloud.thumbnail_jobs.clear();
                self.cloud.thumbnail_active = 0;
                self.cloud.thumbnail_failed.clear();
                self.cloud.selected.clear();
                self.cloud.people = None;
                self.cloud.people_target = None;
                self.cloud.total = 0;
                self.cloud.library_total = None;
                for doc in self.cloud.docs.values_mut() {
                    doc.detach();
                }
                self.cloud_error(t("cloud.error.access_unavailable"));
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
                if subscription_id != self.cloud.folders_watch
                    && subscription_id != self.cloud.buckets_watch
                    && !self
                        .cloud
                        .asset_pages
                        .iter()
                        .any(|page| page.watch == subscription_id)
                {
                    return Ok(());
                }
                if let Some(screening) = snapshot.screening.clone() {
                    self.cloud.screening = screening;
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
                    id => {
                        match self.cloud.apply_asset_snapshot(id, snapshot) {
                            Ok(true) => {}
                            Ok(false) => return Ok(()),
                            Err(error) => {
                                self.cloud.fail_asset_page(id, &error.to_string());
                                return Err(error);
                            }
                        }
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
                }
            }
            Event::WatchError {
                subscription_id,
                error,
            } => {
                if self.cloud.fail_asset_page(&subscription_id, &error)
                    || subscription_id == self.cloud.folders_watch
                    || subscription_id == self.cloud.buckets_watch
                {
                    self.cloud_error(error);
                }
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
                        self.cloud.message = t("cloud.status.updated").into();
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
            .ok_or_else(|| anyhow!(t("cloud.error.unsupported_format")))?
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
                Err(e) => self.cloud_error(tf!("cloud.error.could_not_restore_edits", error = e)),
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
        self.cloud.message = tf!("cloud.status.opening", name = asset.name);
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
            self.cloud_error(t("cloud.error.no_collaborative_editing"));
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
            self.cloud_error(t("cloud.error.connect_before_undo"));
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
        // Filter-stack and transform previews are uncommitted raster caches.
        // Capture after the recipe, placement and pixels commit together.
        if self.stack_filter_session.is_some()
            || self
                .registry
                .tools()
                .find(|tool| tool.id() == self.editor.active_tool)
                .is_some_and(|tool| tool.committed_layer_pixels().is_some())
        {
            return;
        }
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
                self.cloud_error(tf!("cloud.error.edit_not_saved", error = e));
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
            .ok_or_else(|| anyhow!(t("cloud.error.no_cloud_document")))?;
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
            next.active_ink = doc
                .active_ink
                .filter(|id| next.ink_channels.iter().any(|c| c.info.id == *id));
            next.ink_preview = match doc.ink_preview {
                schist_core::InkPreview::Separation(id)
                    if !next.ink_channels.iter().any(|c| c.info.id == id) =>
                {
                    schist_core::InkPreview::Process
                }
                preview => preview,
            };
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
            self.cloud_error(t("cloud.error.no_collaborative_editing_save_local"));
            cx.notify();
            return true;
        }
        self.cloud_capture_edit();
        let remote = &self.cloud.docs[&id];
        if remote.detached {
            self.cloud_error(t("cloud.error.sync_stopped"));
            cx.notify();
            return true;
        }
        if remote.joined && !remote.sending && !remote.changed && remote.saved == remote.generation
        {
            if let Some(doc) = self.cloud_doc_mut(id) {
                doc.mark_saved();
            }
            self.cloud_finish_save(id, cx);
            self.status = t("cloud.status.saved").into();
            cx.notify();
            return true;
        }
        if !self.cloud.docs[&id].joined {
            self.cloud_join(id);
        }
        self.cloud_send_document(id);
        self.status = if self.cloud.connected {
            t("cloud.status.saving")
        } else {
            t("cloud.status.offline_will_sync")
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
            Err(e) => self.cloud_error(tf!("cloud.error.recovery_failed", error = e)),
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
            "" => tf!("cloud.gallery.bucket_n", n = self.cloud.buckets.len() + 1),
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
                fields: vec![("cloud-folder", t("common.folder").into(), String::new())],
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
            self.cloud_error(t("cloud.error.sign_in_first"));
            return None;
        };
        if !self.cloud.connected {
            self.cloud_error(t("cloud.error.wait_for_connection"));
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
                prompt: Some(t("cloud.download.here").into()),
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
                            message: tf!("cloud.download.progress_n_of_m", n = done + 1, m = total),
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
                        message: tn!(
                            "cloud.download.done_to",
                            n as u64,
                            path = crate::ui::shown_path(&dest)
                        ),
                    },
                    Err(e) => Job::Error {
                        epoch,
                        error: tf!("cloud.download.failed_partial", error = e),
                    },
                };
                let _ = sender.send(job);
            });
        })
        .detach();
        self.cloud.message = t("cloud.status.gathering_folder").into();
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
                        progress(tf!(
                            "cloud.download.zipping_n_of_m",
                            n = done + 1,
                            m = total
                        ));
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
                        message: tn!(
                            "cloud.download.zipped_to",
                            n as u64,
                            path = crate::ui::shown_path(&out)
                        ),
                    },
                    Err(e) => Job::Error {
                        epoch,
                        error: tf!("cloud.download.zip_failed", error = e),
                    },
                };
                let _ = sender.send(job);
            });
        })
        .detach();
        self.cloud.message = t("cloud.status.gathering_bucket").into();
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
                anyhow::ensure!(!assets.is_empty(), t("cloud.error.bucket_empty"));
                std::fs::create_dir_all(&dir)?;
                let total = assets.len();
                let mut paths = Vec::with_capacity(total);
                for (done, asset) in assets.into_iter().enumerate() {
                    let _ = sender.send(Job::Done {
                        epoch,
                        message: tf!("cloud.download.fetching_n_of_m", n = done + 1, m = total),
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
                    error: tf!("cloud.download.could_not_fetch_bucket", error = e),
                },
            };
            let _ = sender.send(job);
        });
        self.cloud.message = t("cloud.status.gathering_bucket").into();
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
                    prompt: Some(t("cloud.upload.prompt").into()),
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
        self.cloud.message = t("cloud.upload.uploading_files").into();
        self.cloud.progress = Some((0, 0, t("cloud.upload.looking_through").into()));
        remote::runtime::spawn(async move {
            let progress_sender = sender.clone();
            let report: Report = Arc::new(move |done: u64, total: u64, label: String| {
                let _ = progress_sender.send(Job::Progress {
                    epoch,
                    done,
                    total,
                    label,
                });
            });
            let result: Result<UploadSummary> = (async {
                let files = local_upload_files(&paths)?;
                let uploader =
                    upload_files(&handle, folder.clone(), files, report.clone(), None, None)
                        .await?;
                if let Some(bucket) = bucket {
                    uploader.add_to_bucket(&bucket).await?;
                }
                Ok(UploadSummary {
                    uploaded: uploader.uploaded.len(),
                    existing: uploader.existing.len(),
                    skipped: uploader.skipped,
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
                    error: if e
                        .to_string()
                        .starts_with(t("cloud.error.not_enough_storage"))
                    {
                        e.to_string()
                    } else {
                        tf!("cloud.upload.failed_partial", error = e)
                    },
                },
            };
            let _ = sender.send(job);
        });
        cx.notify();
    }
}
impl Workspace {
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
                fields: vec![("cloud-folder", t("cloud.dialog.folder_id").into(), folder)],
            },
            cx,
        );
    }
    pub(crate) fn cloud_download_selected(&mut self, cx: &mut Context<Self>) {
        if !self.cloud.connected || !self.cloud.capabilities_ready {
            self.cloud_error(t("cloud.download.wait_for_connection"));
            cx.notify();
            return;
        }
        if self.cloud.selected.len() != 1 {
            self.cloud_error(t("cloud.download.select_one"));
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
                fields: vec![(
                    "cloud-download-format",
                    t("cloud.dialog.format").into(),
                    String::new(),
                )],
            },
            cx,
        );
    }
    fn cloud_start_download(&mut self, format: Option<String>) -> Result<()> {
        let asset = self
            .cloud
            .download_target
            .clone()
            .ok_or_else(|| anyhow!(t("cloud.download.select_first")))?;
        let handle = self
            .cloud
            .client
            .as_ref()
            .ok_or_else(|| anyhow!(t("cloud.error.sign_in_first")))?
            .handle
            .clone();
        let capabilities = self.cloud.capabilities.clone();
        let epoch = self.cloud.epoch;
        let sender = self.cloud.sender.clone();
        self.cloud.message = tf!("cloud.download.downloading", name = asset.name);
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
                    error: tf!("cloud.download.failed", error = error),
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
                        ws.cloud.message =
                            tf!("cloud.download.done", name = crate::ui::shown_path(&path));
                        ws.status = ws.cloud.message.clone().into();
                    }
                    Err(error) => {
                        ws.cloud_error(tf!("cloud.download.could_not_save", error = error))
                    }
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
            Ok(()) => self.cloud.message = tf!("cloud.download.done", name = name),
            Err(e) => self.cloud_error(e.to_string()),
        }
        cx.notify();
    }
    fn cloud_upload_current(&mut self, folder: Option<String>) -> Result<()> {
        let doc = self
            .doc
            .as_ref()
            .ok_or_else(|| anyhow!(t("cloud.upload.open_document_first")))?;
        let committed = self.filter_stack_saved_document(doc);
        let doc = committed.as_ref().unwrap_or(doc);
        let data = schist_codec_psd::write_psd(doc)?;
        let id = doc.id;
        let name = format!("{}.psd", doc.title.trim_end_matches(".psd"));
        let handle = self
            .cloud
            .client
            .as_ref()
            .ok_or_else(|| anyhow!(t("cloud.error.sign_in_first")))?
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
            #[cfg(not(target_arch = "wasm32"))]
            "camera-sync" => self.camera_sync_submit(&fields, cx)?,
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
                    anyhow::ensure!(r <= 5, t("cloud.error.rating_range"));
                }
                if let Some(c) = &q.filters.content {
                    anyhow::ensure!(
                        ["all", "safe", "flagged"].contains(&c.as_str()),
                        t("cloud.error.invalid_content_filter")
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
                    .ok_or_else(|| anyhow!(t("cloud.error.no_photo_selected")))?;
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
                        .ok_or_else(|| anyhow!(t("cloud.error.no_bucket_selected")))?;
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
                    .ok_or_else(|| anyhow!(t("cloud.error.no_folder_selected")))?;
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
                    .ok_or_else(|| anyhow!(t("cloud.error.no_item_selected")))?;
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
                anyhow::ensure!(path.is_dir(), t("cloud.error.folder_gone"));
                let folder = get("cloud-folder");
                self.cloud_drop_local(None, (!folder.is_empty()).then_some(folder), vec![path], cx);
            }
            "move-items" => {
                let (bucket, _) = self
                    .cloud
                    .form_target
                    .take()
                    .ok_or_else(|| anyhow!(t("cloud.error.no_bucket_selected")))?;
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
            _ => return Err(anyhow!(t("cloud.error.unknown_action"))),
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
        _ => return Err(anyhow!(t("cloud.error.edited_filter"))),
    };
    let content = match get("cloud-content") {
        "all" | "" => None,
        "safe" => Some("safe".into()),
        "flagged" => Some("flagged".into()),
        _ => return Err(anyhow!(t("cloud.error.content_filter"))),
    };
    let rating = get("cloud-rating");
    let min_rating = if rating.is_empty() {
        None
    } else {
        let r: u8 = rating.parse()?;
        anyhow::ensure!(r <= 5, t("cloud.error.rating_range"));
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
            t("cloud.error.invalid_bounds")
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
        anyhow::ensure!(a <= b, t("cloud.error.date_order"));
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

    fn asset_snapshot(total: u64, offset: u64, ids: &[&str]) -> remote::Snapshot {
        remote::Snapshot {
            screening: None,
            people: None,
            kind: "assets".into(),
            revision: 1,
            total,
            library_asset_count: None,
            offset,
            items: ids.iter().map(|id| value(binding(id).asset)).collect(),
        }
    }

    fn add_page(state: &mut CloudState, offset: u64) -> String {
        let page = AssetPage::new(offset);
        let watch = page.watch.clone();
        state.asset_pages.push(page);
        watch
    }

    #[test]
    fn infinite_scroll_appends_batches_and_waits_for_each_request() {
        let mut state = CloudState::default();
        let first = add_page(&mut state, 0);
        assert_eq!(state.next_asset_offset(), None);
        state
            .apply_asset_snapshot(&first, asset_snapshot(5, 0, &["a", "b"]))
            .unwrap();
        // Respect a provider that returns fewer than PAGE_SIZE photos.
        assert_eq!(state.next_asset_offset(), Some(2));
        state.selected.push("a".into());
        let second = add_page(&mut state, 2);
        assert!(state.is_loading_more());
        assert_eq!(state.next_asset_offset(), None);
        state
            .apply_asset_snapshot(&second, asset_snapshot(5, 2, &["c", "d"]))
            .unwrap();
        assert_eq!(
            state
                .assets
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c", "d"]
        );
        assert_eq!(state.selected, ["a"]);
        assert!(!state.is_loading_more());
        assert_eq!(state.next_asset_offset(), Some(4));
        let last = add_page(&mut state, 4);
        state
            .apply_asset_snapshot(&last, asset_snapshot(5, 4, &["e"]))
            .unwrap();
        assert_eq!(state.next_asset_offset(), None);
    }

    #[test]
    fn infinite_scroll_keeps_live_pages_and_deduplicates_moving_boundaries() {
        let mut state = CloudState::default();
        let first = add_page(&mut state, 0);
        state
            .apply_asset_snapshot(&first, asset_snapshot(5, 0, &["a", "b"]))
            .unwrap();
        let second = add_page(&mut state, 2);
        state
            .apply_asset_snapshot(&second, asset_snapshot(5, 2, &["c", "d"]))
            .unwrap();
        // Deleting the first photo moves c across the page boundary before
        // the second subscription's corresponding update arrives.
        let mut updated = asset_snapshot(4, 0, &["b", "c"]);
        let mut asset = binding("c").asset;
        asset.revision = 2;
        updated.items[1] = value(asset);
        state.apply_asset_snapshot(&first, updated).unwrap();
        assert_eq!(
            state
                .assets
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["b", "c", "d"]
        );
        assert_eq!(state.assets[1].revision, 2);
        state
            .apply_asset_snapshot(&second, asset_snapshot(4, 2, &["d", "e"]))
            .unwrap();
        assert_eq!(
            state
                .assets
                .iter()
                .map(|a| a.id.as_str())
                .collect::<Vec<_>>(),
            ["b", "c", "d", "e"]
        );
        assert_eq!(state.next_asset_offset(), None);
    }

    #[test]
    fn infinite_scroll_stops_on_empty_batches_and_recovers_from_errors() {
        let mut state = CloudState::default();
        let first = add_page(&mut state, 0);
        state
            .apply_asset_snapshot(&first, asset_snapshot(8, 0, &["a", "b"]))
            .unwrap();
        let second = add_page(&mut state, 2);
        assert!(state.fail_asset_page(&second, "offline"));
        assert!(state.loaded);
        assert_eq!(
            state.assets.len(),
            2,
            "earlier results survive a failed batch"
        );
        assert_eq!(state.next_asset_offset(), None);
        assert!(!state.is_loading_more());
        assert!(!state.fail_asset_page("obsolete-query", "stale error"));
        assert!(!state
            .apply_asset_snapshot("obsolete-query", asset_snapshot(0, 0, &[]))
            .unwrap());
        assert_eq!(state.load_error.as_deref(), Some("offline"));
        state
            .apply_asset_snapshot(&second, asset_snapshot(8, 2, &[]))
            .unwrap();
        assert!(state.load_error.is_none());
        assert_eq!(
            state.next_asset_offset(),
            None,
            "an empty batch cannot loop"
        );
        // A later live update can make this page useful again.
        state
            .apply_asset_snapshot(&second, asset_snapshot(8, 2, &["c"]))
            .unwrap();
        assert_eq!(state.next_asset_offset(), Some(3));
    }

    #[test]
    fn infinite_scroll_only_fetches_thumbnails_near_the_viewport() {
        let mut state = CloudState::default();
        state.show = true;
        let asset = binding("photo").asset;
        state.assets = (0..1000)
            .map(|n| Asset {
                id: n.to_string(),
                ..asset.clone()
            })
            .collect();
        state.grid_wanted.extend(["400".into(), "401".into()]);
        assert_eq!(
            state
                .wanted_thumbnails()
                .iter()
                .map(|a| a.0.as_str())
                .collect::<Vec<_>>(),
            ["400", "401"]
        );
        state.grid_wanted.clear();
        state.grid_wanted.insert("800".into());
        assert_eq!(
            state
                .wanted_thumbnails()
                .iter()
                .map(|a| a.0.as_str())
                .collect::<Vec<_>>(),
            ["800"]
        );
    }

    fn people_modal(kind: &'static str, asset: &str) -> Modal {
        Modal::Cloud {
            kind,
            fields: vec![("cloud-asset-id", String::new(), asset.into())],
        }
    }

    #[test]
    fn people_view_keeps_its_photo_and_thumbnail_when_the_gallery_page_changes() {
        let mut state = CloudState::default();
        state.show = true;
        let mut asset = binding("portrait").asset;
        asset.thumbnail_url = Some("https://cloud.test/portrait".into());
        asset.faces = vec![remote::Face {
            id: "face".into(),
            rect: remote::FaceRect {
                x: 0.1,
                y: 0.2,
                w: 0.3,
                h: 0.4,
            },
            person_id: Some("ann".into()),
            automatic: false,
            suggestion: None,
        }];
        state.assets.push(asset.clone());
        state.set_people_modal(Some(&people_modal("people-view", &asset.id)));
        // The page and viewer share one thumbnail request.
        assert_eq!(
            state.wanted_thumbnails(),
            vec![(
                asset.id.clone(),
                asset.revision,
                asset.thumbnail_url.clone()
            )]
        );

        // A live refresh moves the photo off the page. Viewing and naming must
        // still resolve the selected photo, including its face and preview.
        state.assets.clear();
        state.refresh_people_target();
        assert_eq!(state.people_asset("portrait"), Some(&asset));
        assert!(state.people_asset("another-photo").is_none());
        state.set_people_modal(Some(&people_modal("face-name", &asset.id)));
        assert_eq!(state.people_asset("portrait"), Some(&asset));
        assert_eq!(
            state.wanted_thumbnails(),
            vec![(
                asset.id.clone(),
                asset.revision,
                asset.thumbnail_url.clone()
            )]
        );

        // Closing the dialog releases the photo and its thumbnail request.
        state.set_people_modal(None);
        assert!(state.people_asset("portrait").is_none());
        assert!(state.wanted_thumbnails().is_empty());
    }

    #[test]
    fn people_view_retains_updated_faces_and_revision_after_a_later_page_change() {
        let mut state = CloudState::default();
        let mut asset = binding("portrait").asset;
        state.assets.push(asset.clone());
        state.set_people_modal(Some(&people_modal("people-view", &asset.id)));
        asset.revision = 2;
        asset.thumbnail_url = Some("https://cloud.test/updated-portrait".into());
        asset.faces = vec![remote::Face {
            id: "new-face".into(),
            rect: remote::FaceRect {
                x: 0.1,
                y: 0.2,
                w: 0.3,
                h: 0.4,
            },
            person_id: None,
            automatic: true,
            suggestion: None,
        }];
        state.assets = vec![asset.clone()];
        state.refresh_people_target();
        assert_eq!(state.people_asset("portrait"), Some(&asset));

        state.assets = vec![binding("another-photo").asset];
        state.refresh_people_target();
        assert_eq!(state.people_asset("portrait"), Some(&asset));
        assert_eq!(
            state.wanted_thumbnails(),
            vec![(asset.id, 2, asset.thumbnail_url)]
        );
        state.set_people_modal(Some(&people_modal("sign-in", "")));
        assert!(state.people_asset("portrait").is_none());
        assert!(state.wanted_thumbnails().is_empty());
    }

    #[test]
    fn face_previews_preserve_color_and_cache_by_source_and_rectangle() {
        let mut state = CloudState::default();
        let image = rgba_to_render_image(120, 60, [230, 40, 10, 255].repeat(120 * 60)).unwrap();
        let mut rect = remote::FaceRect {
            x: 0.8,
            y: 0.2,
            w: 0.2,
            h: 0.4,
        };
        let crop = state.face_preview_image(&image, &rect).unwrap();
        assert_eq!(u32::from(crop.size(0).width), 64);
        assert_eq!(u32::from(crop.size(0).height), 64);
        // GPUI consumes BGRA, including crops cut from an existing RenderImage.
        assert_eq!(&crop.as_bytes(0).unwrap()[..4], &[10, 40, 230, 255]);
        assert!(Arc::ptr_eq(
            &crop,
            &state.face_preview_image(&image, &rect).unwrap()
        ));
        rect.x = 0.0;
        assert!(!Arc::ptr_eq(
            &crop,
            &state.face_preview_image(&image, &rect).unwrap()
        ));
        let replacement =
            rgba_to_render_image(120, 60, [10, 40, 230, 255].repeat(120 * 60)).unwrap();
        assert_eq!(
            &state
                .face_preview_image(&replacement, &rect)
                .unwrap()
                .as_bytes(0)
                .unwrap()[..4],
            &[230, 40, 10, 255]
        );
        rect.w = f32::NAN;
        assert!(state.face_preview_image(&image, &rect).is_none());
    }
    #[test]
    fn people_thumbnails_load_outside_the_page_and_while_browsing_local_photos() {
        let mut state = CloudState::default();
        state.assets.push(binding("on-page").asset);
        state.people = Some(
            serde_json::from_value(serde_json::json!({
                "enabled": true, "pending": 0, "unnamed": 0,
                "people": [{"id": "ann", "name": "Ann", "asset_count": 1,
                    "avatar": {"asset_id": "off-page", "revision": 7,
                        "thumbnail_url": "https://cloud.test/portrait",
                        "rect": {"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4}}}]
            }))
            .unwrap(),
        );
        let portrait = (
            "off-page".into(),
            7,
            Some("https://cloud.test/portrait".into()),
        );
        assert_eq!(state.wanted_thumbnails(), vec![portrait.clone()]);
        state.show = true;
        state.grid_wanted.insert("on-page".into());
        assert_eq!(
            state.wanted_thumbnails(),
            vec![portrait, ("on-page".into(), 1, None)]
        );
        let avatar = state.people.as_mut().unwrap().people[0]
            .avatar
            .as_mut()
            .unwrap();
        avatar.revision = Some(8);
        assert_eq!(state.wanted_thumbnails()[0].1, 8);
        // Multiple people, the page and stale map markers may share a photo.
        // Fetch it once, using the newest revision and its matching URL.
        let mut source = binding("off-page").asset;
        source.revision = 9;
        source.thumbnail_url = Some("https://cloud.test/new-portrait".into());
        state.assets.push(source.clone());
        state.grid_wanted.insert(source.id.clone());
        source.revision = 7;
        source.thumbnail_url = Some("https://cloud.test/stale-portrait".into());
        state.map_assets.push(source);
        state.map_wanted.insert("off-page".into());
        assert_eq!(
            state.wanted_thumbnails(),
            vec![
                (
                    "off-page".into(),
                    9,
                    Some("https://cloud.test/new-portrait".into())
                ),
                ("on-page".into(), 1, None),
            ]
        );
        state.people = None;
        state.show = false;
        assert!(state.wanted_thumbnails().is_empty());
    }
    #[test]
    fn legacy_people_avatars_use_the_page_thumbnail_when_available() {
        let mut state = CloudState::default();
        let mut asset = binding("photo").asset;
        asset.thumbnail_url = Some("https://cloud.test/thumbnail".into());
        state.assets.push(asset);
        let avatar: remote::PersonAvatar = serde_json::from_value(serde_json::json!({
            "asset_id": "photo", "rect": {"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.4}
        }))
        .unwrap();
        assert_eq!(
            state.avatar_source(&avatar),
            Some((1, Some("https://cloud.test/thumbnail".into())))
        );
        state.assets.clear();
        assert_eq!(state.avatar_source(&avatar), None);
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
