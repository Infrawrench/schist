//! Schist's headless editor and People pipeline, with explicit instance lifetimes.
//!
//! Build the C and WebAssembly distributions with `make library`.
use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::Engine as _;
use schist_core::{color::Depth, Document, IntRect, Layer};
use schist_i18n::t;
use schist_mcp::{Catalog, Scope, SessionCtx};
use schist_plugin_api::{EditorState, ExportOptions, PluginRegistry};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub mod people;
pub use people::{Face, People, PeopleRequest};
#[cfg(not(target_arch = "wasm32"))]
pub mod ffi;
#[cfg(target_arch = "wasm32")]
mod wasm;

/// Maximum JSON request length. Binary imports/model loading use separate APIs.
pub const MAX_REQUEST_BYTES: usize = 24 * 1024 * 1024;

pub struct Session {
    pub document: Document,
    pub editor: EditorState,
    pub registry: PluginRegistry,
}

impl Session {
    fn context(&mut self) -> SessionCtx<'_> {
        SessionCtx::new(&mut self.document, &mut self.editor, &mut self.registry)
    }
}

/// A host-owned application. No logger, panic hook, event loop, environment
/// changes, plugin-directory scan, implicit model download, or background job.
/// Calls on one instance must be serialized; different instances are independent.
pub struct App {
    sessions: BTreeMap<u32, Session>,
    next_session: u32,
    catalog: Option<Catalog>,
    pub people: People,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            sessions: BTreeMap::new(),
            next_session: 1,
            catalog: None,
            people: People::default(),
        }
    }

    fn catalog(&mut self) -> &Catalog {
        self.catalog.get_or_insert_with(|| {
            Catalog::from_registry_scoped(&schist_mcp::session::builtin_registry(), Scope::Active)
        })
    }

    pub fn session(&mut self, id: u32) -> Result<&mut Session> {
        self.sessions
            .get_mut(&id)
            .with_context(|| format!("Unknown session {id}"))
    }

    fn insert(&mut self, mut document: Document, registry: PluginRegistry) -> Result<u32> {
        let id = self.next_session;
        self.next_session = id.checked_add(1).context("Session identifiers exhausted")?;
        document.snapshot_history_source();
        self.sessions.insert(
            id,
            Session {
                document,
                editor: EditorState::default(),
                registry,
            },
        );
        Ok(id)
    }

    pub fn create(&mut self, title: &str, width: u32, height: u32, depth: Depth) -> Result<u32> {
        ensure!(
            width > 0 && height > 0 && width <= 30_000 && height <= 30_000,
            "Invalid document dimensions"
        );
        // Bound eager background allocation independently of each dimension.
        ensure!(
            u64::from(width) * u64::from(height) <= 64 * 1024 * 1024,
            "Document exceeds 64 megapixels"
        );
        let mut document = Document::new(title, width, height, depth);
        let mut background = Layer::new_raster(t("common.background_layer"));
        schist_core::blit_rgba8(
            &mut background.as_raster_mut().unwrap().tiles,
            depth,
            IntRect::from_size(width, height),
            &vec![255; width as usize * height as usize * 4],
        );
        document.push_layer(background);
        document.mark_saved();
        self.insert(document, schist_mcp::session::builtin_registry())
    }

    pub fn import(&mut self, name: &str, bytes: &[u8]) -> Result<u32> {
        let registry = schist_mcp::session::builtin_registry();
        let document = schist_document::import(&registry, bytes, name)?;
        self.insert(document, registry)
    }

    pub fn close(&mut self, id: u32) -> Result<()> {
        self.sessions.remove(&id).context("Unknown session")?;
        Ok(())
    }

    pub fn export(&mut self, id: u32, extension: &str, options: ExportOptions) -> Result<Vec<u8>> {
        ensure!(
            matches!(options.bit_depth, 8 | 16 | 32),
            "Invalid bit depth"
        );
        ensure!((1..=100).contains(&options.quality), "Invalid quality");
        let session = self.session(id)?;
        session.context().refresh_caches();
        let extension = extension.trim_start_matches('.').to_ascii_lowercase();
        let codec = session
            .registry
            .codec_for(&[], Some(&extension))
            .context("Unknown export format")?;
        ensure!(codec.can_export(), "Format is import-only");
        codec.export_with(&session.document, &options)
    }

    pub fn render(&mut self, id: u32, region: Option<IntRect>) -> Result<(IntRect, Vec<u8>)> {
        self.session(id)?.context().render(region)
    }

    /// Execute a registry action (the same names and schemas as the app's MCP
    /// catalog) against a session. Results use MCP content arrays.
    pub fn call(&mut self, id: u32, name: &str, args: &Value) -> Result<Value> {
        ensure!(args.is_object(), "Arguments must be an object");
        ensure!(
            !matches!(name, "save" | "export" | "photoshop_plugins"),
            "Use the library's byte-oriented export API"
        );
        ensure!(
            name != "render" || args.get("path").is_none(),
            "Library rendering does not write files"
        );
        let action = self
            .catalog()
            .action(name)
            .cloned()
            .with_context(|| format!("Unknown action {name}"))?;
        let session = self.session(id)?;
        let result = schist_mcp::dispatch::call_action(&mut session.context(), &action, args, None);
        session.document.take_damage();
        result
    }

    /// JSON entry point shared by C and WebAssembly. Operation names are stable;
    /// action schemas are discoverable through `catalog`.
    pub fn request(&mut self, request: &Value) -> Result<Value> {
        let op = request
            .get("op")
            .and_then(Value::as_str)
            .context("Missing op")?;
        let id = || {
            request
                .get("session")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .context("Invalid session")
        };
        match op {
            "catalog" => {
                let actions: Vec<_> = self.catalog().defs().iter()
                    .filter(|d| !matches!(d["name"].as_str(), Some("save" | "export" | "photoshop_plugins")))
                    .cloned()
                    .map(|mut def| {
                        if def["name"] == "render" {
                            def["inputSchema"]["properties"].as_object_mut().unwrap().remove("path");
                            def["description"] = json!("Composite the document or a region as a PNG image, downscaled to max_dim for viewing.");
                        }
                        def
                    }).collect();
                Ok(
                    json!({"actions": actions, "formats": schist_document::formats(&schist_mcp::session::builtin_registry())}),
                )
            }
            "create" => {
                let dimension = |key| {
                    request
                        .get(key)
                        .and_then(Value::as_u64)
                        .and_then(|v| u32::try_from(v).ok())
                        .with_context(|| format!("Invalid {key}"))
                };
                let depth = match request.get("depth").and_then(Value::as_u64).unwrap_or(8) {
                    8 => Depth::Eight,
                    16 => Depth::Sixteen,
                    32 => Depth::ThirtyTwo,
                    _ => bail!("Depth must be 8, 16 or 32"),
                };
                Ok(
                    json!({"session": self.create(request["title"].as_str().unwrap_or(t("common.untitled")), dimension("width")?, dimension("height")?, depth)?}),
                )
            }
            "close" => {
                self.close(id()?)?;
                Ok(Value::Null)
            }
            "sessions" => Ok(json!(self.sessions.keys().collect::<Vec<_>>())),
            "call" => self.call(
                id()?,
                request["name"].as_str().context("Missing name")?,
                request.get("args").unwrap_or(&json!({})),
            ),
            "import" => Ok(
                json!({"session": self.import(request["name"].as_str().context("Missing name")?, &decode(request)?)?}),
            ),
            "export" => {
                let id = id()?;
                let depth = match self.session(id)?.document.depth {
                    Depth::Eight => 8,
                    Depth::Sixteen => 16,
                    Depth::ThirtyTwo => 32,
                };
                let options = ExportOptions {
                    quality: request["quality"].as_u64().unwrap_or(90).clamp(1, 100) as u8,
                    bit_depth: request["bit_depth"]
                        .as_u64()
                        .unwrap_or(depth)
                        .try_into()
                        .context("Invalid bit depth")?,
                    dither: request["dither"].as_bool().unwrap_or(true),
                };
                let bytes = self.export(
                    id,
                    request["extension"].as_str().context("Missing extension")?,
                    options,
                )?;
                Ok(json!({"data": base64::engine::general_purpose::STANDARD.encode(bytes)}))
            }
            "load_model" => {
                self.people.load_model(
                    request["id"].as_str().context("Missing model id")?,
                    &decode(request)?,
                )?;
                Ok(Value::Null)
            }
            "unload_model" => {
                self.people
                    .unload_model(request["id"].as_str().context("Missing model id")?)?;
                Ok(Value::Null)
            }
            "detect_faces" => {
                let r: PeopleRequest = serde_json::from_value(request.clone())?;
                Ok(serde_json::to_value(
                    self.people.detect(r.width, r.height, &r.rgb)?,
                )?)
            }
            "people" => Ok(serde_json::to_value(
                self.people
                    .process(serde_json::from_value(request.clone())?)?,
            )?),
            _ => bail!("Unknown operation {op}"),
        }
    }

    pub fn request_json(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        ensure!(bytes.len() <= MAX_REQUEST_BYTES, "Request too large");
        let request: Value = serde_json::from_slice(bytes)?;
        Ok(serde_json::to_vec(&self.request(&request)?)?)
    }
}

fn decode(request: &Value) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(
            request["data"]
                .as_str()
                .ok_or_else(|| anyhow!("Missing base64 data"))?,
        )
        .context("Invalid base64 data")
}
